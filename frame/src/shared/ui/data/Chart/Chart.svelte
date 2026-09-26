<script lang="ts">
  import type { Snippet } from "svelte";
  import { expoOut } from "svelte/easing";
  import { scaleBand, scaleLinear, scaleTime } from "d3-scale";
  import { curveMonotoneX } from "d3-shape";
  import {
    Arc,
    Area,
    Axis,
    Bars,
    Chart as Plot,
    Grid,
    Highlight,
    Pie,
    Rect,
    Rule,
    Spline,
    Svg,
    Text,
    Tooltip,
  } from "layerchart/svg";
  import type { AnyScale } from "layerchart/utils/scales.svelte";
  import { duration, reducedMotion } from "$shared/lib/motion";
  import * as m from "$shared/i18n/messages";
  import Readout from "./Readout.svelte";
  import {
    extremes,
    formatValue,
    pointText,
    tickLabel,
    xText,
    type ChartEvidence,
    type ChartKind,
    type ChartSpec,
  } from "./chart";
  import {
    card,
    legendRule,
    plan,
    readout,
    shortValue,
    valuesCsv,
    type Cell,
    type Part,
    type Row,
  } from "./layer";
  let {
    spec,
    title,
    height,
    onevidence,
    glyph,
  }: {
    spec: ChartSpec;
    /** Names the chart for assistive technology and captions the values table. */
    title: string;
    /** A spark's height; without it a spark fills its container. */
    height?: number;
    onevidence?: (reference: ChartEvidence) => void;
    /** A source's mark in the tooltip, drawn by the owner (a favicon); a letter otherwise. */
    glyph?: Snippet<[ChartEvidence]>;
  } = $props();
  const KINDS: Record<ChartKind, () => string> = {
    bars: m.chart_kind_bars,
    stacked: m.chart_kind_stacked,
    line: m.chart_kind_line,
    area: m.chart_kind_area,
    range: m.chart_kind_range,
    donut: m.chart_kind_donut,
    heat: m.chart_kind_heat,
    spark: m.chart_kind_spark,
  };
  const CHAR = 6.5;
  // A chart arrives once over --motion-base; reduced motion draws it settled.
  const still = reducedMotion();
  const arrival = still ? 0 : duration("base");
  const tween = still
    ? ("none" as const)
    : { type: "tween" as const, duration: arrival, easing: expoOut };
  const draw = still ? undefined : { duration: arrival, easing: expoOut };
  let settled = $state(still);
  $effect(() => {
    if (settled) return;
    const timer = setTimeout(() => (settled = true), arrival + 60);
    return () => clearTimeout(timer);
  });

  const spark = $derived(spec.kind === "spark");
  const compact = $derived(!!spec.compact || spark);
  const shape = $derived(plan(spec, m.chart_other()));
  const labels = $derived(card(spec, shape));
  const usable = $derived(
    spec.series.some((series) => series.points.some((point) => point.y !== null)),
  );
  const cartesian = $derived(spec.kind !== "donut" && spec.kind !== "heat" && !spark);
  const trend = $derived(spec.kind === "line" || spec.kind === "area");
  const lying = $derived(shape.horizontal);
  const band = $derived(shape.scale === "band");
  const legend = $derived.by(() => {
    if (spark || spec.kind === "heat") return [];
    const rule = compact ? labels.legend : legendRule(spec.series.length, false);
    if (spec.kind === "donut")
      return compact
        ? []
        : shape.parts.map((part) => ({
            name: part.label,
            color: part.color,
            share: shape.total ? shares.format(part.value / shape.total) : "",
          }));
    if (rule !== "rows") return [];
    return shape.series.map((series) => ({ name: series.label, color: series.color, share: "" }));
  });
  const table = $derived(!spark && (!compact || labels.legend === "table"));
  const shares = new Intl.NumberFormat(undefined, { style: "percent", maximumFractionDigits: 0 });
  const range = $derived(extremes(spec));
  const reading = $derived(
    range
      ? m.chart_extremes({ low: m.chart_extreme(range.low), high: m.chart_extreme(range.high) })
      : "",
  );
  const summary = $derived.by(() => {
    const axes = [spec.x?.label, spec.y?.label].filter((part) => !!part?.trim()).join("; ");
    return [title, axes, `${KINDS[spec.kind]()}${reading ? `; ${reading}` : ""}`]
      .filter(Boolean)
      .join(". ");
  });
  const exact = $derived(
    spec.series.some((series) => series.points.some((point) => point.display !== undefined)),
  );
  const cited = $derived(
    spec.series.some((series) => series.points.some((point) => point.evidence?.length)),
  );

  // Sizes in the chart's own pixels; the width is the container's, measured by the chart once.
  const tickRoom = $derived(
    compact ? 4 : Math.max(24, ...shape.ticks.map((tick) => label(tick).length * CHAR + 10)),
  );
  const nameRoom = $derived(
    Math.min(160, Math.max(...shape.rows.map((row) => row.label.length * CHAR), 0) + 12),
  );
  const rowHeight = $derived(compact ? 20 : Math.max(22, 10 + spec.series.length * 12));
  const plotHeight = $derived.by(() => {
    if (spec.kind === "heat")
      return (
        (compact ? 0 : 24) +
        spec.series.length *
          (compact ? Math.max(6, Math.floor(132 / Math.max(1, spec.series.length))) : 20)
      );
    if (spec.kind === "donut") return compact ? 132 : 200;
    if (lying) return (compact ? 8 : 30) + shape.rows.length * rowHeight;
    return compact ? 132 : 232;
  });
  const padding = $derived.by(() => {
    if (spec.kind === "donut") return { top: 0, right: 0, bottom: 0, left: 0 };
    if (spec.kind === "heat")
      return compact
        ? { top: 0, right: 0, bottom: 0, left: 0 }
        : {
            top: 4,
            right: 0,
            bottom: 20,
            left: Math.max(...spec.series.map((series) => series.name.length * CHAR), 0) + 14,
          };
    if (lying)
      return {
        top: compact ? 4 : 8,
        right: labels.values || !compact ? 44 : 8,
        bottom: compact ? 4 : 22,
        left: nameRoom,
      };
    const labelled = compact && (labels.categories || !!labels.ends);
    return {
      top: compact ? (labels.values ? 16 : 8) : 18,
      right: compact ? (labels.ends ? 36 : 4) : trend ? 12 : 8,
      bottom: compact ? (labelled ? 18 : 4) : 24,
      left: tickRoom,
    };
  });

  function label(tick: number) {
    return tickLabel(tick, spec.y);
  }
  const timeSpan = $derived.by(() => {
    const times = shape.rows.map((row) => (row.x instanceof Date ? row.x.getTime() : 0));
    return Math.max(...times, 0) - Math.min(...times, 0);
  });
  const timeFormat = $derived(
    new Intl.DateTimeFormat(
      undefined,
      timeSpan > 2 * 86_400_000
        ? { month: "short", day: "numeric" }
        : { hour: "numeric", minute: "2-digit" },
    ),
  );
  function category(value: unknown): string {
    if (value instanceof Date) return timeFormat.format(value);
    if (typeof value === "number") return xText(value, "linear");
    return xText(String(value), spec.x?.kind);
  }
  const valueScale = (): AnyScale => scaleLinear();
  const bandScale = (): AnyScale => scaleBand().paddingInner(0.3).paddingOuter(0.15);
  const xScale = $derived<AnyScale>(
    shape.scale === "time" ? scaleTime() : shape.scale === "linear" ? scaleLinear() : bandScale(),
  );
  /** Where a bar's value or a line's last value is written, in plot pixels. */
  function spot(
    context: {
      xScale: AnyScale;
      yScale: AnyScale;
      x1Scale: AnyScale | null;
      y1Scale: AnyScale | null;
    },
    row: Row,
    order: number,
  ): { x: number; y: number; anchor: "start" | "middle" } | null {
    const point = row.points[order];
    if (!point || point.y === null) return null;
    const key = `s${order}`;
    const top = point.y2 ?? point.y;
    const up = top >= 0;
    if (lying) {
      const offset = context.y1Scale
        ? context.y1Scale(key) + (context.y1Scale.bandwidth?.() ?? 0) / 2
        : (context.yScale.bandwidth?.() ?? 0) / 2;
      return {
        x: context.xScale(top) + (up ? 4 : -4),
        y: context.yScale(row.key) + offset + 3.5,
        anchor: "start",
      };
    }
    const at = band ? row.key : row.x;
    const offset = !band
      ? 0
      : context.x1Scale
        ? context.x1Scale(key) + (context.x1Scale.bandwidth?.() ?? 0) / 2
        : (context.xScale.bandwidth?.() ?? 0) / 2;
    return {
      x: context.xScale(at) + offset,
      y: context.yScale(top) + (up ? -4 : 12),
      anchor: "middle",
    };
  }
  const ends = $derived(
    labels.ends && shape.rows.length > 1
      ? [shape.rows[0]!.x, shape.rows.at(-1)!.x]
      : shape.rows.slice(0, 1).map((row) => row.x),
  );
  const lastRow = $derived(shape.rows.findLast((row) => row.points[0] && row.points[0].y !== null));
  const tint = (tone: number | null) =>
    tone === null
      ? "var(--color-fill)"
      : `color-mix(in oklab, var(--color-lit) ${Math.round(6 + tone * 94)}%, var(--color-surface))`;
  const indexOf = (data: unknown): number => {
    const found = data as Partial<Row & Part & Cell> | null;
    return found?.index ?? -1;
  };

  let copied = $state(false);
  function copy() {
    void navigator.clipboard.writeText(valuesCsv(spec)).then(
      () => {
        copied = true;
        setTimeout(() => (copied = false), 1600);
      },
      () => (copied = false),
    );
  }
</script>

{#snippet tip()}
  <Tooltip.Root
    variant="none"
    portal={false}
    pointerEvents
    motion={still ? "none" : "spring"}
    fadeDuration={still ? 0 : duration("fast")}
  >
    {#snippet children({ data })}
      {@const text = readout(spec, shape, indexOf(data))}
      {#if text}<Readout readout={text} {onevidence} {glyph} />{/if}
    {/snippet}
  </Tooltip.Root>
{/snippet}

{#if spark}
  <span class="spark" style:block-size={height ? `${height}px` : undefined}>
    {#if usable}
      <Plot
        data={shape.rows}
        x={band ? "key" : "x"}
        {xScale}
        series={shape.series.slice(0, 1)}
        valueAxis="y"
        yDomain={spec.x?.kind === "category" ? shape.domain : undefined}
        yNice={false}
        bandPadding={0.25}
        padding={{ top: 2, right: 1, bottom: 2, left: 1 }}
        {height}
        tooltipContext={false}
      >
        <Svg role="img" aria-label={summary}>
          {#if spec.x?.kind === "category"}
            <Bars seriesKey="s0" fill={shape.series[0]?.color} motion={tween} class="mark bar" />
          {:else}
            <Spline seriesKey="s0" curve={curveMonotoneX} {draw} class="mark line" />
          {/if}
        </Svg>
      </Plot>
    {/if}
  </span>
{:else}
  <figure class="chart" class:compact>
    {#if usable}
      {#if legend.length}
        <ul class="legend" class:rows={compact}>
          {#each legend as entry, order (order)}
            <li>
              <span class="swatch" style:background={entry.color}
              ></span>{entry.name}{#if entry.share}<span class="share">{entry.share}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      <div class="plot" style:block-size={`${plotHeight}px`}>
        {#if spec.kind === "donut"}
          <Plot
            data={shape.parts}
            x="value"
            height={plotHeight}
            {padding}
            tooltipContext={{ mode: "manual", hideDelay: 160 }}
          >
            {#snippet children({ context })}
              {@const outer = Math.max(8, Math.min(context.width, context.height) / 2 - 4)}
              <Svg center role="img" aria-label={summary}>
                <g class="ring" class:enter={!settled}>
                  <Pie sort={null} padAngle={shape.parts.length > 1 ? 0.012 : 0}>
                    {#snippet children({ arcs })}
                      {#each arcs as slice, index (index)}
                        <Arc
                          startAngle={slice.startAngle}
                          endAngle={slice.endAngle}
                          padAngle={slice.padAngle}
                          innerRadius={outer * 0.64}
                          outerRadius={outer}
                          data={slice.data}
                          fill={(slice.data as Part).color}
                          tooltip
                          class={context.tooltip.data && context.tooltip.data !== slice.data
                            ? "mark slice dim"
                            : "mark slice"}
                        />
                      {/each}
                    {/snippet}
                  </Pie>
                </g>
                {#if compact && labels.lead}
                  <Text value={labels.lead.share} y={-2} textAnchor="middle" class="total" />
                  <Text
                    value={labels.lead.label}
                    y={14}
                    textAnchor="middle"
                    class="caption-text lead"
                  />
                {:else}
                  <Text
                    value={formatValue(shape.total, spec.y)}
                    y={2}
                    textAnchor="middle"
                    class="total"
                  />
                  <Text value={m.chart_total()} y={16} textAnchor="middle" class="caption-text" />
                {/if}
              </Svg>
              {@render tip()}
            {/snippet}
          </Plot>
        {:else if spec.kind === "heat"}
          <Plot
            data={shape.cells}
            x="key"
            xScale={scaleBand().paddingInner(0.08)}
            xDomain={shape.keys}
            y="order"
            yScale={scaleBand().paddingInner(0.1)}
            yDomain={spec.series.map((_, order) => String(order))}
            height={plotHeight}
            {padding}
            tooltipContext={{ mode: "manual", hideDelay: 160 }}
          >
            {#snippet children({ context })}
              <Svg role="img" aria-label={summary}>
                {#each shape.cells as cell (cell.index)}
                  <Rect
                    x={context.xScale(cell.key)}
                    y={context.yScale(cell.order)}
                    width={context.xScale.bandwidth?.() ?? 0}
                    height={context.yScale.bandwidth?.() ?? 0}
                    fill={tint(cell.tone)}
                    class={`mark cell${settled ? "" : " enter"}${context.tooltip.data === cell ? " on" : ""}`}
                    onpointermove={(event: PointerEvent) => context.tooltip.show(event, cell)}
                    onpointerleave={() => context.tooltip.hide()}
                  />
                {/each}
                {#if !compact}
                  <Axis placement="bottom" format={category} tickMarks={false} rule={false} />
                  <Axis
                    placement="left"
                    format={(order: string) => spec.series[Number(order)]?.name ?? ""}
                    tickMarks={false}
                    rule={false}
                    class="names"
                  />
                {/if}
              </Svg>
              {@render tip()}
            {/snippet}
          </Plot>
        {:else if cartesian}
          <Plot
            data={shape.rows}
            x={lying ? undefined : band ? "key" : "x"}
            y={lying ? "key" : undefined}
            xScale={lying ? valueScale() : xScale}
            yScale={lying ? bandScale() : valueScale()}
            xDomain={lying ? shape.domain : band ? shape.keys : undefined}
            yDomain={lying ? shape.keys : shape.domain}
            xNice={false}
            yNice={false}
            valueAxis={lying ? "x" : "y"}
            series={shape.series}
            seriesLayout={shape.layout}
            bandPadding={0.3}
            groupPadding={0.12}
            height={plotHeight}
            {padding}
            tooltipContext={{ mode: band ? "band" : "bisect-x", hideDelay: 160 }}
          >
            {#snippet children({ context })}
              <Svg role="img" aria-label={summary}>
                <Grid
                  x={lying}
                  y={!lying}
                  xTicks={lying ? shape.ticks : undefined}
                  yTicks={lying ? undefined : shape.ticks}
                  class="grid"
                />
                {#if shape.domain[0] < 0 && shape.domain[1] > 0}
                  <Rule x={lying ? 0 : false} y={lying ? false : 0} class="zero" />
                {/if}
                {#if band}<Highlight area />{/if}
                {#if trend}
                  {#each shape.series as series (series.key)}
                    {#if spec.kind === "area"}
                      <Area
                        seriesKey={series.key}
                        y0={() => Math.max(shape.domain[0], Math.min(0, shape.domain[1]))}
                        curve={curveMonotoneX}
                        defined={(row: Row) => row.points[Number(series.key.slice(1))]?.y != null}
                        fill={series.color}
                        class={settled ? "mark area" : "mark area enter"}
                      />
                    {/if}
                    <Spline
                      seriesKey={series.key}
                      curve={curveMonotoneX}
                      defined={(row: Row) => row.points[Number(series.key.slice(1))]?.y != null}
                      stroke={series.color}
                      {draw}
                      class="mark line"
                    />
                  {/each}
                  <Highlight points={{ r: 4 }} lines />
                {:else}
                  {#each shape.series as series (series.key)}
                    <Bars
                      seriesKey={series.key}
                      x1={!lying && shape.layout === "group" ? () => series.key : undefined}
                      y1={lying && shape.layout === "group" ? () => series.key : undefined}
                      rounded={shape.layout === "stackDiverging"
                        ? (row: Row) =>
                            context.series.isStackTop(series.key, row) ? "edge" : "none"
                        : spec.kind === "range"
                          ? "all"
                          : "edge"}
                      radius={4}
                      stackPadding={shape.layout === "stackDiverging" ? 2 : 0}
                      fill={series.color}
                      motion={tween}
                      class="mark bar"
                    />
                    {#if compact ? labels.values : spec.series.length === 1 && shape.layout === "group" && shape.rows.length <= 12}
                      {@const order = Number(series.key.slice(1))}
                      {#each shape.rows as row (row.key)}
                        {@const at = spot(context, row, order)}
                        {#if at}
                          <Text
                            x={at.x}
                            y={at.y}
                            textAnchor={at.anchor}
                            value={compact
                              ? shortValue(row.points[order], spec.y)
                              : pointText(row.points[order], spec.y)}
                            class={settled ? "value" : "value enter"}
                          />
                        {/if}
                      {/each}
                    {/if}
                  {/each}
                {/if}
                {#if !compact}
                  <Axis
                    placement={lying ? "bottom" : "left"}
                    ticks={shape.ticks}
                    format={label}
                    tickMarks={false}
                    rule={false}
                  />
                  <Axis
                    placement={lying ? "left" : "bottom"}
                    format={category}
                    tickMarks={false}
                    rule={false}
                    class={lying ? "names" : undefined}
                  />
                {:else if labels.categories}
                  <Axis
                    placement={lying ? "left" : "bottom"}
                    format={category}
                    tickMarks={false}
                    rule={false}
                    class="names"
                  />
                {:else if labels.ends}
                  <Axis
                    placement="bottom"
                    ticks={ends}
                    format={category}
                    tickMarks={false}
                    rule={false}
                  />
                  {@const at = lastRow && spot(context, lastRow, 0)}
                  {#if at && labels.ends}
                    <Text
                      x={at.x + 6}
                      y={at.y + 4}
                      textAnchor="start"
                      value={labels.ends.value}
                      class="value end"
                    />
                  {/if}
                {/if}
              </Svg>
              {@render tip()}
            {/snippet}
          </Plot>
        {/if}
      </div>
    {:else}<p role="status" class="caption">{m.chart_unavailable()}</p>{/if}
    {#if !compact && reading}<p class="caption reading">{reading}</p>{/if}
    {#if spec.basis?.trim()}<p class="caption">{spec.basis}</p>{/if}
    {#if table}
      <details>
        <summary>{exact ? m.chart_exact_values() : m.chart_values()}</summary>
        {#if exact}<p class="caption">{m.chart_precision()}</p>{/if}
        {#if !compact}
          <button type="button" class="copy" onclick={copy}
            >{copied ? m.chart_copied() : m.chart_copy_values()}</button
          >
        {/if}
        <div class="values">
          <table>
            <caption>{title}</caption>
            <thead>
              <tr>
                <th scope="col">{spec.x?.label ?? ""}</th>
                {#each spec.series as series, order (order)}
                  <th scope="col">{series.name || spec.y?.label || ""}</th>
                {/each}
                {#if cited}<th scope="col">{m.chart_sources()}</th>{/if}
              </tr>
            </thead>
            <tbody>
              {#each shape.rows as row (row.key)}
                <tr>
                  <th scope="row">{row.label}</th>
                  {#each row.points as point, order (order)}<td>{pointText(point, spec.y)}</td
                    >{/each}
                  {#if cited}
                    <td class="cites">
                      {#each row.points.flatMap((point) => point?.evidence ?? []) as reference, index (index)}
                        <button
                          type="button"
                          class="chip"
                          disabled={!onevidence}
                          onclick={() => onevidence?.(reference)}
                          >{reference.origin || reference.label}</button
                        >
                      {/each}
                    </td>
                  {/if}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </details>
    {/if}
  </figure>
{/if}

<style>
  .chart {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    min-inline-size: 0;
    color: var(--color-text);
  }

  .plot {
    position: relative;
    inline-size: 100%;
    min-inline-size: 0;
  }

  .spark {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    min-block-size: 16px;
  }

  /* LayerChart draws; the tokens colour it. */
  .chart :global(.lc-grid-x-rule),
  .chart :global(.lc-grid-y-rule) {
    --stroke-color: var(--color-border);

    shape-rendering: crispedges;
  }

  .chart :global(.zero .lc-rule-x-line),
  .chart :global(.zero .lc-rule-y-line),
  .chart :global(.zero.lc-rule-x-line),
  .chart :global(.zero.lc-rule-y-line) {
    --stroke-color: var(--color-border-strong);
  }

  .chart :global(.lc-axis-tick-label),
  .chart :global(.value),
  .chart :global(.caption-text) {
    --fill-color: var(--color-muted);

    font-size: var(--text-caption);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    stroke: none;
  }

  .chart :global(.lc-highlight-area) {
    --fill-color: var(--color-fill);
  }

  .chart :global(.lc-highlight-line) {
    --stroke-color: var(--color-border-strong);

    stroke-width: 1;
    stroke-dasharray: none;
  }

  .chart :global(.lc-highlight-point) {
    --stroke-color: var(--color-surface);

    stroke-width: 2;
    filter: none;
  }

  .chart :global(.line),
  .spark :global(.line) {
    fill: none;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .spark :global(.line) {
    --stroke-color: var(--chart-1);

    stroke-width: 1.5;
  }

  .chart :global(.area) {
    fill-opacity: 0.1;
    stroke: none;
  }

  .chart :global(.bar) {
    opacity: 0.86;
  }

  .chart :global(.slice) {
    transition: opacity var(--motion-fast) var(--ease-smooth);
  }

  .chart :global(.slice.dim) {
    opacity: 0.4;
  }

  .chart :global(.total) {
    --fill-color: var(--color-text);

    font-size: var(--text-body);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .chart :global(.cell.on) {
    stroke: var(--color-text);
    stroke-width: 1.5;
  }

  .chart :global(.enter) {
    animation: arrive var(--motion-base) var(--ease-emphasized) both;
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 12px;
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  /* The card's legend: at most two rows, filled down then across. */
  .legend.rows {
    display: grid;
    grid-auto-columns: minmax(0, max-content);
    grid-auto-flow: column;
    grid-template-rows: repeat(2, auto);
  }

  .legend li {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .swatch {
    flex: none;
    inline-size: 8px;
    block-size: 8px;
    border-radius: var(--radius-capsule);
  }

  .share {
    color: var(--color-faint);
    font-variant-numeric: tabular-nums;
  }

  .caption,
  summary {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .reading::first-letter {
    text-transform: uppercase;
  }

  details > .caption {
    margin-block: 6px 2px;
  }

  .copy {
    margin-block: 6px 2px;
    padding: 1px 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
  }

  .copy:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .values {
    max-block-size: 280px;
    overflow: auto;
  }

  table {
    inline-size: 100%;
    border-collapse: collapse;
  }

  caption,
  th,
  td {
    padding: 6px 8px;
    text-align: start;
    overflow-wrap: anywhere;
  }

  th,
  td {
    border-block-end: 1px solid var(--color-border);
  }

  td {
    font-variant-numeric: tabular-nums;
  }

  .cites {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .chip {
    max-inline-size: 180px;
    padding: 1px 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  summary:focus-visible,
  .copy:focus-visible,
  .chip:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .chip:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  @keyframes arrive {
    from {
      opacity: 0;
    }
  }
</style>

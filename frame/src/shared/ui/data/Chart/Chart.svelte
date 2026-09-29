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
    ChartCore as Plot,
    Grid,
    Highlight,
    Pie,
    Points,
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
  import Radar from "./Radar.svelte";
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
    ringTop,
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
    detail = "full",
    onevidence,
    glyph,
  }: {
    spec: ChartSpec;
    /** Names the chart for assistive technology and captions the values table. */
    title: string;
    /** A spark's or a tile's height; without it they fill their container. */
    height?: number;
    /**
     * `full` reads every value; `overview` keeps the headline and the marks, with no
     * text too small to read from afar; `tile` is the marks alone.
     */
    detail?: "full" | "overview" | "tile";
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
    radial: m.chart_kind_radial,
    radar: m.chart_kind_radar,
    heat: m.chart_kind_heat,
    spark: m.chart_kind_spark,
  };
  const CHAR = 6.5;
  const uid = $props.id();
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
  /** Text a reader could not make out at this size is not drawn at all. */
  const quiet = $derived(detail !== "full");
  const tile = $derived(detail === "tile");
  const compact = $derived(!!spec.compact || spark || quiet);
  const shape = $derived(plan(spec, m.chart_other()));
  const labels = $derived(card(spec, shape));
  const usable = $derived(
    spec.series.some((series) => series.points.some((point) => point.y !== null)),
  );
  const round = $derived(spec.kind === "donut" || spec.kind === "radial");
  const cartesian = $derived(!round && spec.kind !== "heat" && spec.kind !== "radar" && !spark);
  const trend = $derived(spec.kind === "line" || spec.kind === "area");
  const lying = $derived(shape.horizontal);
  const band = $derived(shape.scale === "band");
  const shares = new Intl.NumberFormat(undefined, { style: "percent", maximumFractionDigits: 0 });
  const legend = $derived.by(() => {
    if (spark || spec.kind === "heat" || quiet) return [];
    if (round)
      return spec.compact
        ? []
        : shape.parts.map((part) => ({
            name: part.label,
            color: part.color,
            share:
              spec.kind === "donut"
                ? shape.total
                  ? shares.format(part.value / shape.total)
                  : ""
                : pointText(part.points[0], spec.y),
          }));
    const rule = spec.compact ? labels.legend : legendRule(spec.series.length, false);
    if (rule !== "rows") return [];
    return shape.series.map((series) => ({ name: series.label, color: series.color, share: "" }));
  });
  const table = $derived(
    !spark && !quiet && spec.values !== false && (!spec.compact || labels.legend === "table"),
  );
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
  /** Bars that carry their own values need no value axis. */
  const valued = $derived(
    !quiet &&
      (spec.compact
        ? labels.values
        : (spec.kind === "bars" || spec.kind === "range") &&
          spec.series.length === 1 &&
          shape.rows.length <= 12),
  );
  const valueAxis = $derived(!compact && !valued);
  /** A line's points are drawn while there are few enough to tell apart. */
  const dotted = $derived(
    spec.kind === "line" && !tile && shape.rows.length <= 16 && spec.series.length <= 3,
  );
  /** The one figure the chart exists to say: the spec's own, or a round chart's total. */
  const headline = $derived(spec.headline);

  // Sizes in the chart's own pixels; the width is the container's, measured by the chart once.
  const tickRoom = $derived(
    Math.max(24, ...shape.ticks.map((tick) => label(tick).length * CHAR + 10)),
  );
  const nameRoom = $derived(
    Math.min(160, Math.max(...shape.rows.map((row) => row.label.length * CHAR), 0) + 12),
  );
  const rowHeight = $derived(compact ? 20 : Math.max(24, 10 + spec.series.length * 12));
  const plotHeight = $derived.by(() => {
    if (tile && height) return height;
    if (spec.kind === "heat")
      return (
        (compact ? 0 : 24) +
        spec.series.length *
          (compact ? Math.max(6, Math.floor(132 / Math.max(1, spec.series.length))) : 20)
      );
    if (round) return tile ? 150 : detail === "overview" ? 260 : spec.compact ? 132 : 188;
    if (spec.kind === "radar") return tile ? 150 : detail === "overview" ? 260 : 280;
    if (lying) return (compact ? 8 : 30) + shape.rows.length * rowHeight;
    if (tile) return 96;
    return detail === "overview" ? 200 : spec.compact ? 132 : 220;
  });
  const padding = $derived.by(() => {
    if (round || tile) return { top: 2, right: 2, bottom: 2, left: 2 };
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
        top: compact ? 4 : 4,
        right: valued || labels.values ? 52 : 8,
        bottom: valueAxis ? 22 : 4,
        left: quiet ? 4 : nameRoom,
      };
    const labelled = spec.compact && !quiet && (labels.categories || !!labels.ends);
    return {
      top: valued || (spec.compact && labels.values && !quiet) ? 20 : 8,
      right: valueAxis
        ? tickRoom + (trend ? 16 : 0)
        : spec.compact && labels.ends && !quiet
          ? 36
          : trend
            ? 12
            : 4,
      bottom: quiet ? 4 : spec.compact ? (labelled ? 18 : 4) : 24,
      left: trend ? 12 : 0,
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
    if (shape.scale === "index") return shape.rows[Number(value)]?.label ?? "";
    if (typeof value === "number") return xText(value, "linear");
    return xText(String(value), spec.x?.kind);
  }
  const valueScale = (): AnyScale => scaleLinear();
  const bandScale = (): AnyScale =>
    scaleBand()
      .paddingInner(tile ? 0.2 : 0.28)
      .paddingOuter(0.1);
  const xScale = $derived<AnyScale>(
    shape.scale === "time" ? scaleTime() : shape.scale === "band" ? bandScale() : scaleLinear(),
  );
  /** Category ticks on an index axis: every one while they fit, else evenly thinned. */
  const indexTicks = $derived.by(() => {
    if (shape.scale !== "index") return undefined;
    const every = Math.max(1, Math.ceil(shape.rows.length / 8));
    return shape.rows.filter((row) => row.index % every === 0).map((row) => row.index);
  });
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
        x: context.xScale(top) + (up ? 6 : -6),
        y: context.yScale(row.key) + offset + 4,
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
      y: context.yScale(top) + (up ? -7 : 14),
      anchor: "middle",
    };
  }
  /** Bars are rounded to their band, never past half of it. */
  function radius(context: { xScale: AnyScale; yScale: AnyScale; x1Scale: AnyScale | null }) {
    const scale = lying ? context.yScale : (context.x1Scale ?? context.xScale);
    const width = scale.bandwidth?.() ?? 12;
    return Math.max(2, Math.min(tile ? 3 : 6, width / 2));
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
  const top = $derived(spec.kind === "radial" ? ringTop(spec, shape.parts) : 1);
  /** What a round chart says in its middle: the headline, else the total or the one value. */
  const middle = $derived.by(() => {
    if (spec.compact && labels.lead) return { value: labels.lead.share, label: labels.lead.label };
    if (headline) return headline;
    if (spec.kind === "radial") {
      const first = shape.parts[0];
      return first ? { value: pointText(first.points[0], spec.y), label: first.label } : null;
    }
    return { value: formatValue(shape.total, spec.y), label: m.chart_total() };
  });

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
  {#if !quiet}
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
  {/if}
{/snippet}

{#snippet washes()}
  <defs>
    {#each shape.series as series, order (series.key)}
      <linearGradient id={`${uid}-wash-${order}`} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" style:stop-color={series.color} style:stop-opacity={tile ? 0.5 : 0.36} />
        <stop offset="100%" style:stop-color={series.color} style:stop-opacity="0.02" />
      </linearGradient>
    {/each}
  </defs>
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
  <figure
    class="chart {detail}"
    class:compact={spec.compact}
    class:round
    class:radar={spec.kind === "radar"}
  >
    {#if headline && !tile && (!round || (spec.kind === "radial" && shape.parts.length > 1))}
      <p class="headline">
        <span class="figure">{headline.value}</span><span class="what">{headline.label}</span>
      </p>
    {/if}
    {#if usable}
      <div class="body" class:beside={round && legend.length > 0}>
        <div
          class="plot"
          style:block-size={`${plotHeight}px`}
          style:inline-size={round ? `${plotHeight}px` : undefined}
        >
          {#if spec.kind === "radar"}
            <Radar {spec} {shape} {detail} {summary} {settled} {onevidence} {glyph} />
          {:else if round}
            <Plot
              data={shape.parts}
              x="value"
              height={plotHeight}
              {padding}
              tooltipContext={quiet ? false : { mode: "manual", hideDelay: 160 }}
            >
              {#snippet children({ context })}
                {@const outer = Math.max(8, Math.min(context.width, context.height) / 2 - 2)}
                <Svg center role="img" aria-label={summary}>
                  <g class="ring" class:enter={!settled}>
                    {#if spec.kind === "donut"}
                      <Pie sort={null} padAngle={shape.parts.length > 1 ? 0.02 : 0}>
                        {#snippet children({ arcs })}
                          {#each arcs as slice, index (index)}
                            <Arc
                              startAngle={slice.startAngle}
                              endAngle={slice.endAngle}
                              padAngle={slice.padAngle}
                              innerRadius={outer * (tile ? 0.58 : 0.66)}
                              outerRadius={outer}
                              cornerRadius={tile ? 2 : 4}
                              data={slice.data}
                              fill={(slice.data as Part).color}
                              tooltip={!quiet}
                              class={context.tooltip.data && context.tooltip.data !== slice.data
                                ? "mark slice dim"
                                : "mark slice"}
                            />
                          {/each}
                        {/snippet}
                      </Pie>
                    {:else}
                      {@const count = Math.max(1, shape.parts.length)}
                      {@const inner = outer * (count === 1 ? 0.72 : 0.36)}
                      {@const step = (outer - inner) / count}
                      {@const thickness = Math.max(3, step * (count === 1 ? 1 : 0.72))}
                      {#each shape.parts as part (part.index)}
                        {@const edge = outer - part.index * step}
                        <Arc
                          value={Math.min(part.value, top)}
                          domain={[0, top]}
                          range={[0, 360]}
                          innerRadius={edge - thickness}
                          outerRadius={edge}
                          cornerRadius={thickness / 2}
                          track={{ class: "track" }}
                          motion={tween}
                          data={part}
                          fill={part.color}
                          tooltip={!quiet}
                          class={context.tooltip.data && context.tooltip.data !== part
                            ? "mark slice dim"
                            : "mark slice"}
                        />
                      {/each}
                    {/if}
                  </g>
                  {#if middle && !tile && (spec.kind === "donut" || shape.parts.length === 1)}
                    <Text
                      value={middle.value}
                      y={detail === "full" ? -1 : 0}
                      textAnchor="middle"
                      class="total"
                    />
                    {#if detail === "full"}<Text
                        value={middle.label}
                        y={spec.compact ? 14 : 17}
                        textAnchor="middle"
                        class={spec.compact ? "caption-text lead" : "caption-text"}
                      />{/if}
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
              tooltipContext={quiet ? false : { mode: "manual", hideDelay: 160 }}
            >
              {#snippet children({ context })}
                <Svg role="img" aria-label={summary}>
                  {#each shape.cells as cell (cell.index)}
                    <Rect
                      x={context.xScale(cell.key)}
                      y={context.yScale(cell.order)}
                      width={context.xScale.bandwidth?.() ?? 0}
                      height={context.yScale.bandwidth?.() ?? 0}
                      rx={2}
                      fill={tint(cell.tone)}
                      class={`mark cell${settled ? "" : " enter"}${context.tooltip.data === cell ? " on" : ""}`}
                      onpointermove={(event: PointerEvent) =>
                        !quiet && context.tooltip.show(event, cell)}
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
              xDomain={lying
                ? shape.domain
                : band
                  ? shape.keys
                  : shape.scale === "index"
                    ? [0, Math.max(1, shape.rows.length - 1)]
                    : undefined}
              yDomain={lying ? shape.keys : shape.domain}
              xNice={false}
              yNice={false}
              valueAxis={lying ? "x" : "y"}
              series={shape.series}
              seriesLayout={shape.layout}
              bandPadding={0.28}
              groupPadding={0.14}
              height={plotHeight}
              {padding}
              tooltipContext={quiet ? false : { mode: band ? "band" : "bisect-x", hideDelay: 160 }}
            >
              {#snippet children({ context })}
                <Svg role="img" aria-label={summary}>
                  {#if trend && spec.kind === "area"}{@render washes()}{/if}
                  {#if !tile && !(lying && valued)}
                    <Grid
                      x={lying}
                      y={!lying}
                      xTicks={lying ? shape.ticks : undefined}
                      yTicks={lying ? undefined : shape.ticks}
                      class="grid"
                    />
                  {/if}
                  {#if shape.domain[0] < 0 && shape.domain[1] > 0}
                    <Rule x={lying ? 0 : false} y={lying ? false : 0} class="zero" />
                  {/if}
                  {#if band && !quiet}<Highlight area />{/if}
                  {#if trend}
                    {#each shape.series as series, order (series.key)}
                      {#if spec.kind === "area"}
                        <Area
                          seriesKey={series.key}
                          y0={shape.layout === "stack"
                            ? undefined
                            : () => Math.max(shape.domain[0], Math.min(0, shape.domain[1]))}
                          curve={curveMonotoneX}
                          defined={(row: Row) => row.points[order]?.y != null}
                          fill={`url(#${uid}-wash-${order})`}
                          line={{
                            stroke: series.color,
                            class: "mark line",
                            draw,
                          }}
                          class={settled ? "mark area" : "mark area enter"}
                        />
                      {:else}
                        <Spline
                          seriesKey={series.key}
                          curve={curveMonotoneX}
                          defined={(row: Row) => row.points[order]?.y != null}
                          stroke={series.color}
                          {draw}
                          class="mark line"
                        />
                        {#if dotted}
                          <Points
                            seriesKey={series.key}
                            r={3.5}
                            fill={series.color}
                            class={settled ? "mark dot" : "mark dot enter"}
                          />
                        {/if}
                      {/if}
                    {/each}
                    {#if !quiet}<Highlight points={{ r: 4 }} lines />{/if}
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
                        radius={radius(context)}
                        stackPadding={shape.layout === "stackDiverging" ? 2 : 0}
                        fill={series.color}
                        motion={tween}
                        class="mark bar"
                      />
                      {#if valued}
                        {@const order = Number(series.key.slice(1))}
                        {#each shape.rows as row (row.key)}
                          {@const at = spot(context, row, order)}
                          {#if at}
                            <Text
                              x={at.x}
                              y={at.y}
                              textAnchor={at.anchor}
                              value={spec.compact
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
                    {#if valueAxis}
                      <Axis
                        placement={lying ? "bottom" : "right"}
                        ticks={shape.ticks}
                        format={label}
                        tickMarks={false}
                        rule={false}
                      />
                    {/if}
                    <Axis
                      placement={lying ? "left" : "bottom"}
                      ticks={indexTicks}
                      format={category}
                      tickMarks={false}
                      rule={false}
                      class={lying ? "names" : undefined}
                    />
                  {:else if quiet}
                    <!-- Nothing written: the marks and the headline carry it. -->
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
        {#if legend.length}
          <ul class="legend" class:rows={spec.compact} class:list={round}>
            {#each legend as entry, order (order)}
              <li>
                <span class="swatch" style:background={entry.color}></span><span class="name"
                  >{entry.name}</span
                >{#if entry.share}<span class="share">{entry.share}</span>{/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {:else}<p role="status" class="caption">{m.chart_unavailable()}</p>{/if}
    {#if spec.basis?.trim() && !quiet}<p class="caption basis">{spec.basis}</p>{/if}
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
    gap: 12px;
    margin: 0;
    min-inline-size: 0;
    color: var(--color-text);
  }

  .chart.tile {
    gap: 0;
    block-size: 100%;
  }

  .headline {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
  }

  .figure {
    font-size: var(--text-title);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
    line-height: 28px;
  }

  .what {
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .overview .figure {
    font-size: var(--text-overview-figure);
    line-height: 1.1;
  }

  .overview .what {
    font-size: var(--text-overview-label);
    line-height: 1.25;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-inline-size: 0;
  }

  .round .body,
  .radar .body {
    align-items: center;
  }

  .body.beside {
    flex-direction: row;
    gap: 28px;
  }

  .plot {
    position: relative;
    flex: none;
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
  .chart :global(.caption-text) {
    --fill-color: var(--color-muted);

    font-size: var(--text-caption);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    stroke: none;
  }

  .chart :global(.value) {
    --fill-color: var(--color-label-secondary);

    font-size: var(--text-caption);
    font-weight: 550;
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

  .tile :global(.line) {
    stroke-width: 2.5;
  }

  .spark :global(.line) {
    --stroke-color: var(--chart-1);

    stroke-width: 1.5;
  }

  .chart :global(.area) {
    stroke: none;
  }

  .chart :global(.dot) {
    stroke: var(--color-surface);
    stroke-width: 1.5;
  }

  .chart :global(.slice) {
    transition: opacity var(--motion-fast) var(--ease-smooth);
  }

  .chart :global(.slice.dim) {
    opacity: 0.4;
  }

  .chart :global(.track) {
    fill: var(--color-fill);
  }

  .chart :global(.total) {
    --fill-color: var(--color-text);

    font-size: var(--text-title);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
    dominant-baseline: central;
  }

  .compact :global(.total) {
    font-size: var(--text-body);
    font-weight: 600;
  }

  .overview :global(.total) {
    font-size: var(--text-overview-figure);
  }

  .overview :global(.caption-text) {
    font-size: var(--text-overview-label);
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
    gap: 6px 16px;
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

  /* A round chart names its parts beside it, each with its share or value. */
  .legend.list {
    display: grid;
    flex: 1;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 9px 8px;
    min-inline-size: 0;
    font-size: var(--text-label);
  }

  .legend li {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-inline-size: 0;
  }

  .legend.list li {
    display: contents;
  }

  .name {
    min-inline-size: 0;
    overflow-wrap: anywhere;
  }

  .legend.list .name {
    color: var(--color-label-secondary);
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

  .legend.list .share {
    padding-inline-start: 12px;
    color: var(--color-text);
    font-weight: 550;
    text-align: end;
  }

  .caption,
  summary {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .basis {
    color: var(--color-faint);
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
    max-inline-size: 240px;
    padding: 1px 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    overflow-wrap: anywhere;
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

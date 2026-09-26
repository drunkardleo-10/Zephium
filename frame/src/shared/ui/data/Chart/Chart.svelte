<script lang="ts">
  import type { Snippet } from "svelte";
  import { duration, reducedMotion } from "$shared/lib/motion";
  import * as m from "$shared/i18n/messages";
  import Axes from "./Axes.svelte";
  import Tooltip from "./Tooltip.svelte";
  import Bars from "./marks/Bars.svelte";
  import Donut from "./marks/Donut.svelte";
  import Heat from "./marks/Heat.svelte";
  import Line from "./marks/Line.svelte";
  import {
    extremes,
    formatValue,
    pointText,
    seriesColor,
    xText,
    type ChartEvidence,
    type ChartKind,
    type ChartSpec,
  } from "./chart";
  import { categoryKeys, geometry, readout } from "./geometry";
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
    /** A spark's height; every other kind sizes itself from the container's width. */
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
  let width = $state(0);
  let boxHeight = $state(0);
  let hovered = $state<number | null>(null);
  // A chart arrives once; reduced motion draws it settled.
  let settled = $state(reducedMotion());
  $effect(() => {
    if (settled) return;
    const timer = setTimeout(() => (settled = true), duration("slow") + 60);
    return () => clearTimeout(timer);
  });
  const spark = $derived(spec.kind === "spark");
  const compact = $derived(!!spec.compact || spark);
  const board = $derived(Math.max(spark ? 24 : 200, width || (spark ? 96 : 320)));
  const usable = $derived(
    spec.series.some((series) => series.points.some((point) => point.y !== null)),
  );
  const shape = $derived(
    geometry(spec, board, { other: m.chart_other(), sparkHeight: height ?? (boxHeight || 24) }),
  );
  const tip = $derived(
    hovered === null || spark ? null : readout(spec, shape, hovered, m.chart_other()),
  );
  const anchor = $derived.by(() => {
    if (hovered === null) return null;
    const point =
      spec.kind === "donut"
        ? shape.slices[hovered]?.anchor
        : shape.hits.find((hit) => hit.index === hovered)?.anchor;
    if (!point) return null;
    const left = (point.x / shape.width) * 100;
    return {
      left,
      top: (point.y / shape.height) * 100,
      align: left < 25 ? "start" : left > 75 ? "end" : "middle",
      below: point.y < 40,
    } as const;
  });
  const cursor = $derived(
    shape.cursor && hovered !== null ? shape.hits.find((hit) => hit.index === hovered) : undefined,
  );
  const legend = $derived.by(() => {
    if (compact) return [];
    if (spec.kind === "donut")
      return shape.slices.map((slice) => ({
        name: slice.label,
        color: slice.color,
        share: shape.total ? formatShare(slice.value / shape.total) : "",
      }));
    if (spec.kind === "heat" || spec.series.length < 2) return [];
    return spec.series.map((series, order) => ({
      name: series.name,
      color: seriesColor(order),
      share: "",
    }));
  });
  const shares = new Intl.NumberFormat(undefined, { style: "percent", maximumFractionDigits: 0 });
  const formatShare = (value: number) => shares.format(value);
  const summary = $derived.by(() => {
    const axes = [spec.x?.label, spec.y?.label].filter((part) => !!part?.trim()).join("; ");
    const range = extremes(spec);
    const reading = range
      ? `; ${m.chart_extremes({
          low: m.chart_extreme(range.low),
          high: m.chart_extreme(range.high),
        })}`
      : "";
    return [title, axes, `${KINDS[spec.kind]()}${reading}`].filter(Boolean).join(". ");
  });
  const keys = $derived(categoryKeys(spec));
  const exact = $derived(
    spec.series.some((series) => series.points.some((point) => point.display !== undefined)),
  );
  const cited = $derived(
    spec.series.some((series) => series.points.some((point) => point.evidence?.length)),
  );
  const rowsFor = (key: string) =>
    spec.series.map((series) => series.points.find((point) => String(point.x) === key));
  const total = $derived(spec.kind === "donut" ? formatValue(shape.total ?? 0, spec.y) : "");
</script>

{#if spark}
  <span class="spark" bind:clientWidth={width} bind:clientHeight={boxHeight}>
    <svg viewBox={`0 0 ${shape.width} ${shape.height}`} role="img" aria-label={summary}>
      <Bars bars={shape.bars} enter={!settled} />
      <Line traces={shape.traces} enter={!settled} />
    </svg>
  </span>
{:else}
  <figure class="chart" class:compact>
    {#if usable}
      {#if legend.length}
        <ul class="legend">
          {#each legend as entry, order (order)}
            <li>
              <span class="swatch" style:background={entry.color}
              ></span>{entry.name}{#if entry.share}<span class="share">{entry.share}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      <div
        class="plot"
        bind:clientWidth={width}
        role="presentation"
        onpointerleave={() => (hovered = null)}
      >
        <svg
          viewBox={`0 0 ${shape.width} ${shape.height}`}
          width={shape.width}
          height={shape.height}
          role="img"
          aria-label={summary}
        >
          <Axes {shape} />
          {#if cursor}
            <line
              class="cursor"
              x1={cursor.anchor.x}
              x2={cursor.anchor.x}
              y1={shape.plot.top}
              y2={shape.plot.bottom}
            />
          {/if}
          {#if spec.kind === "donut" && shape.centre}
            <Donut
              slices={shape.slices}
              centre={shape.centre}
              {total}
              caption={m.chart_total()}
              {hovered}
              enter={!settled}
              onhover={(index) => (hovered = index)}
            />
          {:else if spec.kind === "heat"}
            <Heat cells={shape.cells} {hovered} enter={!settled} />
          {:else}
            <Bars bars={shape.bars} {hovered} enter={!settled} />
            <Line traces={shape.traces} {hovered} enter={!settled} />
          {/if}
          {#each shape.hits as hit (hit.index)}
            <rect
              class="hit"
              role="presentation"
              x={hit.x}
              y={hit.y}
              width={hit.width}
              height={hit.height}
              onpointerenter={() => (hovered = hit.index)}
            />
          {/each}
        </svg>
        {#if tip && anchor}
          <Tooltip readout={tip} {...anchor} {onevidence} {glyph} />
        {/if}
      </div>
    {:else}<p role="status" class="caption">{m.chart_unavailable()}</p>{/if}
    {#if spec.basis?.trim()}<p class="caption">{spec.basis}</p>{/if}
    {#if !compact}
      <details>
        <summary>{exact ? m.chart_exact_values() : m.chart_values()}</summary>
        {#if exact}<p class="caption">{m.chart_precision()}</p>{/if}
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
              {#each keys as key (key)}
                {@const points = rowsFor(key)}
                <tr>
                  <th scope="row">{xText(key, spec.x?.kind)}</th>
                  {#each points as point, order (order)}<td>{pointText(point, spec.y)}</td>{/each}
                  {#if cited}
                    <td class="cites">
                      {#each points.flatMap((point) => point?.evidence ?? []) as reference, index (index)}
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

  svg {
    display: block;
    inline-size: 100%;
    block-size: auto;
    overflow: visible;
  }

  .spark {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    min-block-size: 16px;
  }

  .hit {
    fill: transparent;
  }

  .cursor {
    stroke: var(--color-border-strong);
    stroke-width: 1;
    pointer-events: none;
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

  .legend li {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .swatch {
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

  details > .caption {
    margin-block: 6px 2px;
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
  .chip:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .chip:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>

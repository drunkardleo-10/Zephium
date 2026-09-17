<script lang="ts">
  import {
    axisTicks,
    basisText,
    plotPoints,
    pointEvidence,
    tickLabel,
    type ChartBasis,
    type ChartEvidence,
    type ChartSeries,
  } from "./chart";
  import * as m from "$shared/i18n/messages";
  let {
    title,
    xLabel,
    yLabel,
    series,
    basis,
    generalKnowledge = false,
    compact = false,
    onevidence,
  }: {
    title: string;
    xLabel: string;
    yLabel: string;
    series: readonly ChartSeries[];
    /** What the numbers were measured under; stated beside the plot. */
    basis?: ChartBasis;
    generalKnowledge?: boolean;
    /** The card version: the plot alone, without the readout or the values. */
    compact?: boolean;
    onevidence?: (reference: ChartEvidence) => void;
  } = $props();
  const TINTS = [
    "var(--color-accent)",
    "var(--color-tint-sky)",
    "var(--color-tint-sage)",
    "var(--color-tint-rose)",
    "var(--color-warning)",
    "var(--color-tint-graphite)",
  ];
  let width = $state(0);
  let hovered = $state<number | null>(null);
  const plotted = $derived(
    series.map((entry) => ({ name: entry.name, points: plotPoints(entry.points) })),
  );
  const usable = $derived(
    plotted.every((entry) => entry.points !== null) &&
      plotted.some((entry) => (entry.points?.length ?? 0) > 0),
  );
  /** Labels come from the longest series; every series shares the band. */
  const labels = $derived(
    usable
      ? (plotted.reduce(
          (longest, entry) =>
            (entry.points?.length ?? 0) > longest.length ? entry.points! : longest,
          [] as { label: string; value: number }[],
        ) ?? [])
      : [],
  );
  const mode = $derived(series.length > 1 || labels.length > 14 ? "line" : "bar");
  const height = $derived(compact ? 132 : 232);
  const pad = $derived(
    compact
      ? { top: 12, right: 8, bottom: 18, left: 34 }
      : { top: 18, right: 12, bottom: 26, left: 44 },
  );
  const board = $derived(Math.max(200, width || 320));
  const inner = $derived({
    width: Math.max(40, board - pad.left - pad.right),
    height: Math.max(40, height - pad.top - pad.bottom),
  });
  const ticks = $derived.by(() => {
    const values = plotted.flatMap((entry) => entry.points?.map((point) => point.value) ?? []);
    return values.length
      ? axisTicks(Math.min(...values), Math.max(...values), compact ? 3 : 5)
      : [];
  });
  const scale = $derived.by(() => {
    const low = ticks.length ? ticks[0]! : 0;
    const high = ticks.length ? ticks[ticks.length - 1]! : 1;
    const span = high - low || 1;
    return {
      low,
      high,
      y: (value: number) => pad.top + inner.height - ((value - low) / span) * inner.height,
    };
  });
  const band = $derived(labels.length ? inner.width / labels.length : inner.width);
  const centre = (index: number) => pad.left + band * (index + 0.5);
  const barWidth = $derived(Math.max(4, Math.min(compact ? 22 : 44, band * 0.62)));
  /** Only as many x labels as fit; the values table keeps every one of them. */
  const labelStep = $derived(Math.max(1, Math.ceil(labels.length / Math.floor(inner.width / 56))));
  const showValues = $derived(!compact && mode === "bar" && labels.length <= 10);
  const chips = $derived(hovered === null ? [] : pointEvidence(series, hovered));
  const readout = $derived.by(() => {
    const index = hovered;
    if (index === null) return null;
    return {
      label: labels[index]?.label ?? "",
      values: series.map((entry, order) => ({
        name: entry.name,
        value: entry.points[index]?.value ?? "",
        tint: TINTS[order % TINTS.length]!,
      })),
    };
  });
  /** Which band the pointer is over, in the plot's own coordinates. */
  function track(event: PointerEvent & { currentTarget: SVGSVGElement }) {
    const box = event.currentTarget.getBoundingClientRect();
    if (!labels.length || !box.width) return;
    const x = ((event.clientX - box.left) / box.width) * board;
    const index = Math.floor((x - pad.left) / band);
    hovered = index >= 0 && index < labels.length ? index : null;
  }
  const basisLine = $derived(basisText(basis));
  const summary = $derived(
    `${title}. ${xLabel}; ${yLabel}. ${series.map((entry) => entry.name).join(", ")}`,
  );
</script>

<figure class="chart" class:compact>
  {#if usable}
    {#if series.length > 1}
      <ul class="legend">
        {#each series as entry, order (order)}
          <li>
            <span class="swatch" style:background={TINTS[order % TINTS.length]}></span>{entry.name}
          </li>
        {/each}
      </ul>
    {/if}
    <div class="plot" bind:clientWidth={width}>
      <svg
        viewBox={`0 0 ${board} ${height}`}
        width={board}
        {height}
        role="img"
        aria-label={summary}
        onpointermove={track}
        onpointerleave={() => (hovered = null)}
      >
        {#each ticks as tick, index (index)}
          <line
            class="grid"
            x1={pad.left}
            x2={pad.left + inner.width}
            y1={scale.y(tick)}
            y2={scale.y(tick)}
            class:zero={tick === 0}
          />
          <text class="tick" x={pad.left - 6} y={scale.y(tick) + 3} text-anchor="end"
            >{tickLabel(tick)}</text
          >
        {/each}
        {#if mode === "bar"}
          {#each labels as point, index (point.label)}
            {@const top = Math.min(scale.y(point.value), scale.y(0))}
            {@const size = Math.max(1, Math.abs(scale.y(point.value) - scale.y(0)))}
            <rect
              class="bar"
              class:on={hovered === index}
              x={centre(index) - barWidth / 2}
              y={top}
              width={barWidth}
              height={size}
              rx="3"
            />
            {#if showValues}
              <text class="value" x={centre(index)} y={top - 6} text-anchor="middle"
                >{series[0]?.points[index]?.value ?? ""}</text
              >
            {/if}
          {/each}
        {:else}
          {#each plotted as entry, order (order)}
            {@const points = entry.points ?? []}
            <polyline
              class="line"
              style:stroke={TINTS[order % TINTS.length]}
              points={points
                .map((point, index) => `${centre(index)},${scale.y(point.value)}`)
                .join(" ")}
            />
            {#each points as point, index (index)}
              <circle
                class="dot"
                class:on={hovered === index}
                style:fill={TINTS[order % TINTS.length]}
                cx={centre(index)}
                cy={scale.y(point.value)}
                r={hovered === index ? 3.5 : 2.5}
              />
            {/each}
          {/each}
        {/if}
        {#each labels as point, index (point.label)}
          {#if index % labelStep === 0}
            <text class="tick" x={centre(index)} y={height - pad.bottom + 14} text-anchor="middle"
              >{point.label}</text
            >
          {/if}
        {/each}
        {#if hovered !== null}
          <line
            class="cursor"
            x1={centre(hovered)}
            x2={centre(hovered)}
            y1={pad.top}
            y2={pad.top + inner.height}
          />
        {/if}
      </svg>
    </div>
    {#if !compact}
      <div class="readout" aria-live="polite">
        {#if readout}
          <span class="point">{readout.label}</span>
          {#each readout.values as entry, order (order)}
            <span class="reading"
              ><span class="swatch" style:background={entry.tint}></span>{entry.value}</span
            >
          {/each}
          {#each chips as reference (reference.key)}
            <button type="button" class="chip" onclick={() => onevidence?.(reference)}
              >{reference.origin || reference.label}</button
            >
          {/each}
        {:else}<span class="axes">{xLabel} · {yLabel}</span>{/if}
      </div>
    {/if}
  {:else}<p role="status" class="unavailable">{m.work_chart_unavailable()}</p>{/if}
  {#if basisLine}<p class="basis">{m.work_chart_basis({ basis: basisLine })}</p>{/if}
  {#if generalKnowledge}<p class="basis">{m.work_general_knowledge()}</p>{/if}
  {#if !compact}
    <details>
      <summary>{m.work_chart_values()}</summary>
      <div class="values">
        <table>
          <caption>{title}</caption>
          <thead
            ><tr
              ><th scope="col">{xLabel}</th>{#each series as entry, order (order)}<th scope="col"
                  >{entry.name || yLabel}</th
                >{/each}<th scope="col">{m.work_sources()}</th></tr
            ></thead
          >
          <tbody>
            {#each series[0]?.points ?? [] as point, index (index)}
              <tr>
                <th scope="row">{point.label}</th>
                {#each series as entry, order (order)}<td>{entry.points[index]?.value ?? ""}</td
                  >{/each}
                <td class="cites">
                  {#each point.evidence ?? [] as reference (reference.key)}
                    <button type="button" class="chip" onclick={() => onevidence?.(reference)}
                      >{reference.origin || reference.label}</button
                    >
                  {/each}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </details>
  {/if}
</figure>

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
    inline-size: 100%;
    min-inline-size: 0;
  }

  svg {
    display: block;
    inline-size: 100%;
    block-size: auto;
    overflow: visible;
  }

  .grid {
    stroke: var(--color-border);
    stroke-width: 1;
  }

  .grid.zero {
    stroke: var(--color-border-strong);
  }

  .tick {
    fill: var(--color-faint);
    font-size: 10px;
  }

  .value {
    fill: var(--color-muted);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }

  .bar {
    fill: var(--color-accent);
    opacity: 0.75;
    transition: opacity var(--motion-fast) var(--ease-smooth);
  }

  .bar.on {
    opacity: 1;
  }

  .line {
    fill: none;
    stroke-width: 1.75;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .dot {
    transition: r var(--motion-fast) var(--ease-smooth);
  }

  .cursor {
    stroke: var(--color-border-strong);
    stroke-width: 1;
    stroke-dasharray: 3 3;
  }

  .legend,
  .readout {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
    min-block-size: 20px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .legend li,
  .reading {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .swatch {
    inline-size: 8px;
    block-size: 8px;
    border-radius: 2px;
  }

  .point {
    color: var(--color-text);
    font-weight: 550;
  }

  .reading {
    font-variant-numeric: tabular-nums;
  }

  .chip {
    padding: 1px 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    max-inline-size: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chip:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .basis,
  .unavailable,
  .axes,
  summary {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
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
    text-align: start;
    padding: 6px 8px;
    overflow-wrap: anywhere;
  }

  td {
    font-variant-numeric: tabular-nums;
  }

  th,
  td {
    border-block-end: 1px solid var(--color-border);
  }

  .cites {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  summary:focus-visible,
  .chip:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>

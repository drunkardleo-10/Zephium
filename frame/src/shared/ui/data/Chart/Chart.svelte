<script lang="ts">
  import { BarChart } from "layerchart/svg";
  import { plotPoints, type ChartSeries } from "./chart";
  import * as m from "$shared/i18n/messages";
  let {
    title,
    xLabel,
    yLabel,
    series,
  }: { title: string; xLabel: string; yLabel: string; series: readonly ChartSeries[] } = $props();
  let selected = $state(0);
  let current = $derived(series[Math.min(selected, Math.max(0, series.length - 1))]);
  let data = $derived(current ? plotPoints(current.points) : []);
</script>

<div class="chart">
  {#if series.length > 1}<label
      >{m.work_chart_series()}<select bind:value={selected}
        >{#each series as item, i (i)}<option value={i}>{item.name}</option>{/each}</select
      ></label
    >{/if}
  {#if data && data.length}
    <div
      class="plot"
      role="img"
      aria-label={`${title}: ${current?.name ?? ""}. ${xLabel}; ${yLabel}`}
    >
      <BarChart
        {data}
        x="label"
        y="value"
        motion="none"
        tooltipContext={false}
        highlight={false}
        rule={false}
        series={[{ key: "value", value: "value", color: "var(--color-accent)" }]}
      />
    </div>
    <p>{m.work_chart_precision()}</p>
  {:else}<p role="status">{m.work_chart_unavailable()}</p>{/if}
  <details>
    <summary>{m.work_chart_values()}</summary>
    <div class="values">
      <table>
        <caption>{current?.name ?? title}</caption><thead
          ><tr><th scope="col">{xLabel}</th><th scope="col">{yLabel}</th></tr></thead
        ><tbody
          >{#each current?.points ?? [] as point, i (i)}<tr
              ><th scope="row">{point.label}</th><td>{point.value}</td></tr
            >{/each}</tbody
        >
      </table>
    </div>
  </details>
</div>

<style>
  .chart {
    color: var(--color-text);
  }

  .plot {
    height: 280px;
    width: 100%;

    --color-primary: var(--color-accent);
    --color-surface-100: var(--color-fill);
    --color-surface-content: var(--color-text);
  }

  p,
  label,
  summary {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  label {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  select {
    padding: 6px;
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font: inherit;
  }

  .values {
    max-height: 280px;
    overflow: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  caption,
  th,
  td {
    text-align: start;
    padding: 8px;
    overflow-wrap: anywhere;
  }

  td {
    font-variant-numeric: tabular-nums;
  }

  th,
  td {
    border-block-end: 1px solid var(--color-border);
  }

  summary:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>

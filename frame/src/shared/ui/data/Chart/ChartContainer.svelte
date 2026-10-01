<script lang="ts">
  import type { Snippet } from "svelte";
  import { setChartContext, type ChartConfig } from "./chart-utils";

  /**
   * shadcn-svelte's ChartContainer: the one element every chart draws in. It
   * publishes the config as colour properties and styles what LayerChart
   * draws with the design tokens.
   */
  let {
    config,
    class: className = "",
    children,
  }: { config: ChartConfig; class?: string; children: Snippet } = $props();

  const id = $props.id();
  setChartContext({
    get config() {
      return config;
    },
  });
  const colors = $derived(
    Object.entries(config)
      .map(([key, entry]) => `--color-${key}: ${entry.color}`)
      .join("; "),
  );
</script>

<div data-chart={id} class="chart-container {className}" style={colors}>
  {@render children()}
</div>

<style>
  .chart-container {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-inline-size: 0;
    color: var(--color-text);
  }

  /* The grid: dashed hairlines, quieter than any mark. */
  .chart-container :global(.lc-grid-x-rule),
  .chart-container :global(.lc-grid-y-rule) {
    --stroke-color: var(--color-border);

    stroke-dasharray: 3 3;
    shape-rendering: crispedges;
  }

  .chart-container :global(.zero .lc-rule-x-line),
  .chart-container :global(.zero .lc-rule-y-line),
  .chart-container :global(.zero.lc-rule-x-line),
  .chart-container :global(.zero.lc-rule-y-line) {
    --stroke-color: var(--color-border-strong);
  }

  .chart-container :global(.lc-axis-tick-label),
  .chart-container :global(.caption-text) {
    --fill-color: var(--color-muted);

    font-size: var(--text-caption);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    stroke: none;
  }

  .chart-container :global(.value) {
    --fill-color: var(--color-label-secondary);

    font-size: var(--text-caption);
    font-weight: 550;
    font-variant-numeric: tabular-nums;
    stroke: none;
  }

  /* The cursor: a soft band under a category, a hairline through a moment. */
  .chart-container :global(.lc-highlight-area) {
    --fill-color: var(--color-fill);
  }

  .chart-container :global(.lc-highlight-line) {
    --stroke-color: var(--color-border-strong);

    stroke-width: 1;
    stroke-dasharray: none;
  }

  .chart-container :global(.lc-highlight-point) {
    --stroke-color: var(--color-surface);

    stroke-width: 2;
    filter: none;
  }
</style>

<script lang="ts">
  import { useChart } from "./chart-utils";

  /**
   * shadcn-svelte's ChartLegend, vendored: a square of each key's colour and
   * its label, in a row under the plot, or as a list beside a round chart
   * with each part's share.
   */
  let {
    keys,
    shares = {},
    layout = "row",
  }: {
    keys: readonly string[];
    /** A part's share or value, written after its name. */
    shares?: Readonly<Record<string, string>>;
    layout?: "row" | "rows" | "list";
  } = $props();

  const chart = useChart();
</script>

<ul class="chart-legend {layout}">
  {#each keys as key (key)}
    {@const entry = chart.config[key]}
    {#if entry}
      <li>
        <span class="swatch" style:background={`var(--color-${key})`}></span><span class="name"
          >{entry.label}</span
        >{#if shares[key]}<span class="share">{shares[key]}</span>{/if}
      </li>
    {/if}
  {/each}
</ul>

<style>
  .chart-legend {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 6px 16px;
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  /* A card's legend: at most two rows, filled down then across. */
  .rows {
    display: grid;
    grid-auto-columns: minmax(0, max-content);
    grid-auto-flow: column;
    grid-template-rows: repeat(2, auto);
    justify-content: start;
  }

  /* A round chart names its parts beside it, each with its share or value. */
  .list {
    display: grid;
    flex: 1;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 9px 8px;
    justify-content: stretch;
    color: var(--color-text);
    font-size: var(--text-label);
  }

  li {
    display: flex;
    align-items: center;
    gap: 6px;
    min-inline-size: 0;
  }

  .list li {
    display: contents;
  }

  .swatch {
    flex: none;
    inline-size: 8px;
    block-size: 8px;
    border-radius: var(--radius-swatch);
  }

  .list .swatch {
    inline-size: 10px;
    block-size: 10px;
  }

  .name {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .share {
    color: var(--color-muted);
    font-variant-numeric: tabular-nums;
    text-align: end;
  }

  .list .share {
    color: var(--color-text);
    font-weight: 550;
  }
</style>

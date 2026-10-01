<script lang="ts">
  import LazyView from "$shared/ui/LazyView";
  import { loadChart } from "$shared/ui/data/Chart";
  import { basisText, workChartSpec } from "$shared/ui/data/Artifact/work-chart";
  import Table from "./Table.svelte";
  import type { BoardActions } from "../../lib/canvas-context";
  import type { ChartBlock } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let {
    block,
    width,
    actions,
    open = false,
  }: { block: ChartBlock; width: number; actions?: BoardActions; open?: boolean } = $props();
  const CHAR = 6.5;
  const spec = $derived.by(() => {
    const basis = basisText(block.chart.basis);
    const base = workChartSpec({
      ...block.chart,
      basis: basis && m.work_chart_basis({ basis }),
    });
    // Names that would run into each other under their bars lie the bars down.
    const names = new Set(
      base.series.flatMap((series) => series.points.map((point) => String(point.x))),
    );
    const band = (width - 64) / Math.max(1, names.size);
    const longest = Math.max(0, ...[...names].map((name) => name.length));
    const lying = longest * CHAR > band - 8 ? { horizontal: true } : {};
    // A chart with its table shows that table as its values, not a second one of its own.
    return { ...base, ...lying, ...(block.values ? { values: false as const } : {}) };
  });
  const values = $derived(
    block.values
      ? {
          id: block.id,
          kind: "table" as const,
          emphasis: block.emphasis,
          state: block.state,
          columns: block.values.columns,
          rows: block.values.rows,
        }
      : null,
  );
</script>

{#if block.headline}<p class="headline">
    <span class="figure">{block.headline.value}</span><span class="what"
      >{block.headline.label}</span
    >
  </p>{/if}
<LazyView
  loader={loadChart}
  loadingLabel={m.surface_loading()}
  failureLabel={m.work_artifact_unavailable()}
  retryLabel={m.surface_retry()}
  >{#snippet children(Plot)}<Plot title={block.title ?? ""} {spec} />{/snippet}</LazyView
>
{#if values}
  {#if open}<div class="values"><Table block={values} open within /></div>{/if}
  <footer>
    <button type="button" class="more nodrag nopan" onclick={() => actions?.toggle(block.id)}
      >{open ? m.work_board_fewer() : m.work_board_values()}</button
    >
  </footer>
{/if}

<style>
  .headline {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0 0 12px;
  }

  .figure {
    font-size: var(--text-title);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
    line-height: 28px;
  }

  .what {
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .values {
    margin-block-start: 16px;
    padding-block-start: 12px;
    border-block-start: 1px solid var(--color-border);
  }

  footer {
    margin-block-start: 12px;
  }

  .more {
    block-size: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .more:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>

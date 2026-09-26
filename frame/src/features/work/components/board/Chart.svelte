<script lang="ts">
  import LazyView from "$shared/ui/LazyView";
  import { loadChart } from "$shared/ui/data/Chart";
  import { basisText, workChartSpec } from "$shared/ui/data/Artifact/work-chart";
  import type { ChartBlock } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let { block, width }: { block: ChartBlock; width: number } = $props();
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
    return longest * CHAR > band - 8 ? { ...base, horizontal: true } : base;
  });
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
</style>

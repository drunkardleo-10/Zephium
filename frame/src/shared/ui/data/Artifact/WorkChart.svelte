<script lang="ts">
  import type { Snippet } from "svelte";
  import Chart from "../Chart/Chart.svelte";
  import type { ChartEvidence } from "../Chart";
  import type { ArtifactContent, EvidenceReference } from "./artifact";
  import { basisText, workChartSpec } from "./work-chart";
  import * as m from "$shared/i18n/messages";
  let {
    title,
    chart,
    compact = false,
    onevidence,
    glyph,
  }: {
    title: string;
    chart: Extract<ArtifactContent, { kind: "chart" }>;
    /** The card: no basis, no legend, no values table. */
    compact?: boolean;
    onevidence?: (reference: EvidenceReference) => void;
    glyph?: Snippet<[ChartEvidence]>;
  } = $props();
  const spec = $derived.by(() => {
    const basis = basisText(chart.basis);
    return workChartSpec({
      ...chart,
      basis: basis && m.work_chart_basis({ basis }),
      compact,
    });
  });
  /** The chart hands back a key; the owner's own reference is what opens. */
  function cite(reference: ChartEvidence) {
    for (const series of chart.series)
      for (const point of series.points) {
        const found = point.evidence?.find((entry) => entry.key === reference.key);
        if (found) return onevidence?.(found);
      }
  }
</script>

<Chart {title} {spec} {glyph} onevidence={onevidence ? cite : undefined} />

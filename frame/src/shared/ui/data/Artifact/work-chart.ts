import { parseValue, type ChartEvidence, type ChartPoint, type ChartSpec } from "../Chart";
import type { EvidenceReference, MeasurementBasisView, WorkChartSeriesView } from "./artifact";

/** A series this long reads as a trend rather than as bars to compare. */
const TREND = 24;

/** The stated basis as one line; an unstated basis has no line at all. */
export function basisText(basis: MeasurementBasisView | undefined): string {
  if (!basis) return "";
  return [basis.method, basis.conditions, basis.versions, basis.observedAt]
    .map((part) => part?.trim())
    .filter((part): part is string => !!part)
    .join(" · ");
}

const evidence = (reference: EvidenceReference): ChartEvidence => ({
  key: reference.key,
  label: reference.label,
  ...(reference.origin ? { origin: reference.origin } : {}),
  ...(reference.url ? { url: reference.url } : {}),
  ...(reference.file ? { file: true } : {}),
});

/**
 * Work's chart on the shared spec. Values arrive as exact decimal strings and are
 * converted once; the original string stays with each point for the values table.
 */
export function workChartSpec(chart: {
  xLabel: string;
  yLabel: string;
  series: readonly WorkChartSeriesView[];
  basis?: string;
  generalKnowledge?: boolean;
  compact?: boolean;
}): ChartSpec {
  const currencies = new Set<string | undefined>();
  let percent = true;
  let ranged = false;
  let longest = 0;
  const series = chart.series.map((entry) => {
    // Duplicate labels would merge into one misleading bar; such a series is not plotted.
    const distinct = new Set(entry.points.map((point) => point.label)).size === entry.points.length;
    longest = Math.max(longest, entry.points.length);
    return {
      name: entry.name,
      points: entry.points.map((point): ChartPoint => {
        const parsed = distinct ? parseValue(point.value) : null;
        if (parsed) {
          currencies.add(parsed.currency);
          percent &&= !!parsed.percent;
          ranged ||= parsed.y2 !== undefined;
        }
        return {
          x: point.label,
          y: parsed?.y ?? null,
          ...(parsed?.y2 !== undefined ? { y2: parsed.y2 } : {}),
          display: point.value,
          ...(point.evidence?.length ? { evidence: point.evidence.map(evidence) } : {}),
        };
      }),
    };
  });
  const [currency] = currencies;
  const money = currencies.size === 1 && currency !== undefined;
  const plotted = currencies.size > 0;
  return {
    kind: ranged ? "range" : longest > TREND ? "line" : "bars",
    series,
    x: { label: chart.xLabel, kind: "category" },
    y: {
      label: chart.yLabel,
      ...(money
        ? { format: "money" as const, currency }
        : plotted && percent
          ? { format: "percent" as const }
          : { format: "number" as const }),
    },
    ...(chart.basis && !chart.compact ? { basis: chart.basis } : {}),
    ...(chart.generalKnowledge ? { knowledge: true } : {}),
    ...(chart.compact ? { compact: true } : {}),
  };
}

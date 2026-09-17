/** Rendering inputs only; the owner resolves evidence identities and links. */
export type ChartEvidence = { key: string; label: string; origin?: string; url?: string };
export type ChartPoint = { label: string; value: string; evidence?: readonly ChartEvidence[] };
export type ChartSeries = { name: string; points: readonly ChartPoint[] };
/** What the numbers were measured under; stated with the plot, never implied. */
export type ChartBasis = {
  method: string;
  conditions?: string;
  versions?: string;
  observedAt?: string;
};
/** Plot coordinates are approximate; always retain the original decimal in the value table. */
export function plotPoints(
  points: readonly ChartPoint[],
): { label: string; value: number }[] | null {
  if (points.length > 256 || new Set(points.map((point) => point.label)).size !== points.length)
    return null;
  const result: { label: string; value: number }[] = [];
  for (const point of points) {
    if (!/^[+-]?\d+(\.\d+)?$/u.test(point.value) || point.value.length > 128) return null;
    const value = Number(point.value);
    if (
      !Number.isFinite(value) ||
      Math.abs(value) > Number.MAX_SAFE_INTEGER ||
      (value === 0 && /[1-9]/u.test(point.value))
    )
      return null;
    result.push({ label: point.label, value });
  }
  return result;
}

/** Round axis bounds with human steps, so a reader can add a tick in their head. */
export function axisTicks(min: number, max: number, count = 5): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max) || count < 2) return [];
  const low = Math.min(0, min);
  const high = Math.max(0, max);
  if (low === high) return [low];
  const step = niceStep((high - low) / (count - 1));
  const first = Math.floor(low / step) * step;
  const last = Math.ceil(high / step) * step;
  const ticks: number[] = [];
  for (let value = first; value <= last + step / 2 && ticks.length <= 32; value += step)
    ticks.push(Math.abs(value) < step / 1e6 ? 0 : Number(value.toPrecision(12)));
  return ticks;
}

function niceStep(raw: number): number {
  const magnitude = 10 ** Math.floor(Math.log10(Math.abs(raw) || 1));
  const scaled = Math.abs(raw) / magnitude;
  const step = scaled <= 1 ? 1 : scaled <= 2 ? 2 : scaled <= 5 ? 5 : 10;
  return step * magnitude;
}

/** A tick label that keeps large numbers readable without inventing precision. */
export function tickLabel(value: number): string {
  const size = Math.abs(value);
  if (size >= 1_000_000) return `${trim(value / 1_000_000)}M`;
  if (size >= 1_000) return `${trim(value / 1_000)}k`;
  return trim(value);
}

function trim(value: number): string {
  return Number(value.toFixed(2)).toLocaleString();
}

/** The stated basis as one line; an unstated basis has no line at all. */
export function basisText(basis: ChartBasis | undefined): string {
  if (!basis) return "";
  return [basis.method, basis.conditions, basis.versions, basis.observedAt]
    .map((part) => part?.trim())
    .filter((part): part is string => !!part)
    .join(" · ");
}

/** Distinct references across every series at one band, in the order shown. */
export function pointEvidence(series: readonly ChartSeries[], index: number): ChartEvidence[] {
  const out: ChartEvidence[] = [];
  for (const entry of series)
    for (const reference of entry.points[index]?.evidence ?? [])
      if (!out.some((known) => known.key === reference.key)) out.push(reference);
  return out;
}

export type ChartSeries = { name: string; points: readonly ChartPoint[] };
type ChartPoint = { label: string; value: string };
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

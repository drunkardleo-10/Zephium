/** One chart spec for every caller. Rendering inputs only; the owner resolves evidence. */
export type ChartKind = "bars" | "stacked" | "line" | "area" | "range" | "donut" | "heat" | "spark";
export type ChartEvidence = {
  key: string;
  label: string;
  origin?: string;
  url?: string;
  file?: boolean;
};
export type ChartPoint = {
  /** A category, a number, or an ISO date for a time axis. */
  x: string | number;
  /** Null is a gap: nothing is drawn and nothing is invented. */
  y: number | null;
  /** The top of a range; the bar spans y to y2. */
  y2?: number;
  /** The value as its source wrote it; the values table shows it verbatim. */
  display?: string;
  evidence?: readonly ChartEvidence[];
};
export type ChartSeries = { name: string; points: readonly ChartPoint[] };
export type ChartFormat = "number" | "money" | "duration" | "percent" | "bytes";
export type ChartX = { label?: string; kind?: "category" | "time" | "linear" };
export type ChartY = {
  label?: string;
  unit?: string;
  format?: ChartFormat;
  currency?: string;
  min?: number;
  max?: number;
};
export type ChartSpec = {
  kind: ChartKind;
  series: readonly ChartSeries[];
  x?: ChartX;
  y?: ChartY;
  /** One line, drawn under the plot when present. */
  basis?: string;
  /** The numbers come from what the agent knows, not a source; nothing draws it. */
  knowledge?: boolean;
  /** The card size: no axis labels, no legend, values on hover only. */
  compact?: boolean;
  /** Bars lie down whatever their names: the owner knows their names would collide. */
  horizontal?: boolean;
};

/** Categorical slots; a ninth series takes the neutral tone rather than a new hue. */
export function seriesColor(order: number): string {
  return order < 8 ? `var(--chart-${order + 1})` : "var(--color-tint-graphite)";
}

// Parsing: a value as a person or an agent writes it.

const NUMBER = String.raw`[+-]?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?`;
const SYMBOL = String.raw`[$€£¥]`;
const ONE = new RegExp(`^(${SYMBOL})?\\s*(${NUMBER})\\s*(%)?$`, "u");
const RANGE = new RegExp(
  `^(${SYMBOL})?\\s*(${NUMBER})\\s*(%)?\\s*(?:-|–|—|to)\\s*(${SYMBOL})?\\s*(${NUMBER})\\s*(%)?$`,
  "u",
);
const CURRENCY: Record<string, string> = { $: "USD", "€": "EUR", "£": "GBP", "¥": "JPY" };
export type ParsedValue = { y: number; y2?: number; currency?: string; percent?: boolean };

/** A plain or ranged decimal, or null when a plot would misstate it. */
export function parseValue(text: string): ParsedValue | null {
  const value = text.trim();
  if (!value || value.length > 128) return null;
  const one = ONE.exec(value);
  if (one) {
    const y = exact(one[2]!);
    return y === null ? null : withMarks({ y }, one[1], !!one[3]);
  }
  const range = RANGE.exec(value);
  if (!range) return null;
  const low = exact(range[2]!);
  const high = exact(range[5]!);
  const symbol = range[1] ?? range[4];
  if (low === null || high === null) return null;
  if (range[1] && range[4] && range[1] !== range[4]) return null;
  return withMarks(
    { y: Math.min(low, high), y2: Math.max(low, high) },
    symbol,
    !!(range[3] || range[6]),
  );
}

function withMarks(value: ParsedValue, symbol: string | undefined, percent: boolean): ParsedValue {
  if (symbol) value.currency = CURRENCY[symbol];
  if (percent) value.percent = true;
  return value;
}

function exact(text: string): number | null {
  const digits = text.replaceAll(",", "");
  const value = Number(digits);
  if (
    !Number.isFinite(value) ||
    Math.abs(value) > Number.MAX_SAFE_INTEGER ||
    (value === 0 && /[1-9]/u.test(digits))
  )
    return null;
  return value;
}

// Formatting: tabular numerals, and a unit a reader recognises.

const numbers = new Map<string, Intl.NumberFormat>();
function numberFormat(options: Intl.NumberFormatOptions): Intl.NumberFormat {
  const key = JSON.stringify(options);
  let format = numbers.get(key);
  if (!format) numbers.set(key, (format = new Intl.NumberFormat(undefined, options)));
  return format;
}

/** A value as the reader should see it; `short` is the axis tick form. */
export function formatValue(value: number, y: ChartY = {}, short = false): string {
  const notation = short ? "compact" : "standard";
  switch (y.format) {
    case "money":
      if (!y.currency) break;
      try {
        return numberFormat({
          style: "currency",
          currency: y.currency,
          notation,
          maximumFractionDigits: short ? 1 : Number.isInteger(value) ? 0 : 2,
          minimumFractionDigits: short || Number.isInteger(value) ? 0 : 2,
        }).format(value);
      } catch {
        break;
      }
    case "duration":
      return formatDuration(value);
    case "percent":
      return numberFormat({ style: "percent", maximumFractionDigits: 1 }).format(value / 100);
    case "bytes":
      return formatBytes(value);
  }
  const text = numberFormat({ notation, maximumFractionDigits: 2 }).format(value);
  return y.unit && !short ? `${text} ${y.unit}` : text;
}

/** Seconds as a person says them: 45s, 12m, 1h 24m. */
export function formatDuration(seconds: number): string {
  const sign = seconds < 0 ? "-" : "";
  const total = Math.round(Math.abs(seconds));
  if (total < 60) return `${sign}${total}s`;
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  if (hours) return `${sign}${hours}h${minutes ? ` ${minutes}m` : ""}`;
  return `${sign}${minutes}m${rest && minutes < 10 ? ` ${rest}s` : ""}`;
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];
/** Decimal units, as the platform's own file sizes read: 1.2 GB. */
export function formatBytes(bytes: number): string {
  let value = Math.abs(bytes);
  let unit = 0;
  while (value >= 1000 && unit < BYTE_UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  const text = numberFormat({ maximumFractionDigits: digits }).format(value);
  return `${bytes < 0 ? "-" : ""}${text} ${BYTE_UNITS[unit]}`;
}

/** A point's value as text: its own words first, then y, or y to y2 for a range. */
export function pointText(point: ChartPoint | undefined, y?: ChartY): string {
  if (!point) return "";
  if (point.display !== undefined) return point.display;
  if (point.y === null) return "—";
  const low = formatValue(point.y, y);
  return point.y2 === undefined ? low : `${low}–${formatValue(point.y2, y)}`;
}

/** The label a category or a moment is read by. */
export function xText(x: string | number, kind: ChartX["kind"]): string {
  if (kind === "time") {
    const time = typeof x === "number" ? x : parseTime(x);
    if (Number.isFinite(time)) return dayFormat.format(time);
  }
  return typeof x === "number" ? numberFormat({ maximumFractionDigits: 2 }).format(x) : x;
}
/** An ISO moment in epoch ms; a bare date is that day's local midnight, not UTC's. */
export function parseTime(text: string): number {
  const day = /^(\d{4})-(\d{2})-(\d{2})$/u.exec(text);
  return day
    ? new Date(Number(day[1]), Number(day[2]) - 1, Number(day[3])).getTime()
    : Date.parse(text);
}
const dayFormat = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });

// Axes.

/** Round axis bounds with human steps, so a reader can add a tick in their head. */
export function axisTicks(min: number, max: number, count = 5, zero = true): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max) || count < 2) return [];
  const low = zero ? Math.min(0, min) : min;
  const high = zero ? Math.max(0, max) : max;
  if (low === high) return zero ? [low] : axisTicks(low - 1, high + 1, count, false);
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

const CLOCK = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600, 43200];
/** Duration ticks land on clock steps (15m, 1h), never on 33m 20s. */
export function durationTicks(min: number, max: number, count = 5): number[] {
  const low = Math.min(0, min);
  const high = Math.max(0, max);
  if (low === high) return [0];
  const raw = (high - low) / (count - 1);
  const step = CLOCK.find((candidate) => candidate >= raw) ?? Math.ceil(raw / 86400) * 86400;
  const ticks: number[] = [];
  for (let value = Math.floor(low / step) * step; value <= high + step / 2; value += step)
    ticks.push(value);
  return ticks;
}

/** A tick label that keeps large numbers readable without inventing precision. */
export function tickLabel(value: number, y: ChartY = {}): string {
  return formatValue(value, y, true);
}

/** Distinct references across every series at one category, in the order shown. */
export function pointEvidence(points: readonly (ChartPoint | undefined)[]): ChartEvidence[] {
  const out: ChartEvidence[] = [];
  for (const point of points)
    for (const reference of point?.evidence ?? [])
      if (!out.some((known) => known.key === reference.key)) out.push(reference);
  return out;
}

/** The lowest and highest plotted values with where they are, for the accessible name. */
export function extremes(
  spec: ChartSpec,
): { low: { x: string; value: string }; high: { x: string; value: string } } | null {
  let low: { point: ChartPoint; value: number } | null = null;
  let high: { point: ChartPoint; value: number } | null = null;
  for (const series of spec.series)
    for (const point of series.points) {
      if (point.y === null) continue;
      if (!low || point.y < low.value) low = { point, value: point.y };
      const top = point.y2 ?? point.y;
      if (!high || top > high.value) high = { point, value: top };
    }
  if (!low || !high) return null;
  const read = (entry: { point: ChartPoint; value: number }) => ({
    x: xText(entry.point.x, spec.x?.kind),
    value: formatValue(entry.value, spec.y),
  });
  return { low: read(low), high: read(high) };
}

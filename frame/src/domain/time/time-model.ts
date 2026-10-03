import type { TimeCall } from "$shared/ipc/bindings";

const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;

/** Wall-clock milliseconds as the local zone reads them. Native keeps time in
 *  local hours on the same scale, so a day is always 24 of them. */
export function localMs(date: Date): number {
  return date.getTime() - date.getTimezoneOffset() * 60_000;
}

/** Local days since the epoch. */
export function localDay(date: Date): number {
  return Math.floor(localMs(date) / DAY_MS);
}

/** Local midnight of a local day. */
export function dateOfDay(day: number): Date {
  const utc = new Date(day * DAY_MS);
  return new Date(utc.getUTCFullYear(), utc.getUTCMonth(), utc.getUTCDate());
}

/** 1 for Monday through 7 for Sunday, as the reader's locale counts weeks. */
export function firstDayOfWeek(language = globalThis.navigator?.language): number {
  try {
    const locale = new Intl.Locale(language ?? "en") as Intl.Locale & {
      getWeekInfo?: () => { firstDay: number };
      weekInfo?: { firstDay: number };
    };
    const first = (locale.getWeekInfo?.() ?? locale.weekInfo)?.firstDay;
    return first !== undefined && first >= 1 && first <= 7 ? first : 1;
  } catch {
    return 1;
  }
}

/** The first local day of the week holding `day`. Day 0 was a Thursday. */
export function weekStart(day: number, firstDay = firstDayOfWeek()): number {
  const weekday = ((((day + 3) % 7) + 7) % 7) + 1;
  return day - ((weekday - firstDay + 7) % 7);
}

export type Span = "day" | "week";

/** What a Time surface shows: one day, or the week starting on `start`. */
export interface Period {
  span: Span;
  start: number;
}

export function currentPeriod(span: Span, now = new Date()): Period {
  const today = localDay(now);
  return { span, start: span === "day" ? today : weekStart(today) };
}

export function shiftPeriod(period: Period, by: number): Period {
  return { span: period.span, start: period.start + by * (period.span === "day" ? 1 : 7) };
}

function bucketCount(span: Span): number {
  return span === "day" ? 24 : 7;
}

export function reportCall(period: Period, site: string | null = null): TimeCall {
  return {
    kind: "report",
    from_hour: period.start * 24,
    bucket_hours: period.span === "day" ? 1 : 24,
    buckets: bucketCount(period.span),
    site,
  };
}

/** Which bucket of the period is happening now, or -1 when it is not now. */
export function liveBucket(period: Period, now = new Date()): number {
  const ms = localMs(now);
  const index =
    period.span === "day"
      ? Math.floor((ms - period.start * DAY_MS) / HOUR_MS)
      : Math.floor(ms / DAY_MS) - period.start;
  return index >= 0 && index < bucketCount(period.span) ? index : -1;
}

/** Buckets that have begun, so an average over a running period counts only
 *  those, never the empty hours still ahead. */
export function elapsedBuckets(period: Period, now = new Date()): number {
  const live = liveBucket(period, now);
  if (live >= 0) return live + 1;
  return localMs(now) < period.start * DAY_MS ? 0 : bucketCount(period.span);
}

const hours = new Intl.NumberFormat(undefined, {
  style: "unit",
  unit: "hour",
  unitDisplay: "narrow",
});
const minutes = new Intl.NumberFormat(undefined, {
  style: "unit",
  unit: "minute",
  unitDisplay: "narrow",
});
const seconds = new Intl.NumberFormat(undefined, {
  style: "unit",
  unit: "second",
  unitDisplay: "narrow",
});

/** "4h 52m", "38m", "45s": the two largest units that say something. */
export function duration(totalSeconds: number): string {
  const whole = Math.max(0, Math.round(totalSeconds));
  if (whole < 60) return whole === 0 ? minutes.format(0) : seconds.format(whole);
  const totalMinutes = Math.round(whole / 60);
  const h = Math.floor(totalMinutes / 60);
  const m = totalMinutes % 60;
  if (h === 0) return minutes.format(m);
  return m === 0 ? hours.format(h) : `${hours.format(h)} ${minutes.format(m)}`;
}

/** A countdown as a clock reads it: "18:04", or "1:02:09" past an hour. */
export function countdown(totalSeconds: number): string {
  const whole = Math.max(0, Math.ceil(totalSeconds));
  const h = Math.floor(whole / 3600);
  const m = Math.floor((whole % 3600) / 60);
  const s = whole % 60;
  const pad = (value: number) => String(value).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** One letter a weekday is known by, for a narrow axis. */
export function weekdayLetter(day: number): string {
  return new Intl.DateTimeFormat(undefined, { weekday: "narrow" }).format(dateOfDay(day));
}

export function weekdayShort(day: number): string {
  return new Intl.DateTimeFormat(undefined, { weekday: "short" }).format(dateOfDay(day));
}

/** An hour of the day as the locale writes it: "9 AM", "21". */
export function hourLabel(hour: number): string {
  return new Intl.DateTimeFormat(undefined, { hour: "numeric" }).format(new Date(2000, 0, 1, hour));
}

export function dayLabel(day: number): string {
  return new Intl.DateTimeFormat(undefined, {
    weekday: "long",
    month: "short",
    day: "numeric",
  }).format(dateOfDay(day));
}

export function weekLabel(start: number): string {
  const format = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
  return format.formatRange(dateOfDay(start), dateOfDay(start + 6));
}

/** Short and long break minutes for a round length, as native sets them. */
export function breakMinutes(minutes: number): { short: number; long: number } {
  const short = Math.min(20, Math.max(3, Math.floor(minutes / 5)));
  return { short, long: Math.min(30, short * 3) };
}

/** Seconds as a share of a whole, clamped to 0..1. */
export function share(part: number, whole: number): number {
  return whole > 0 ? Math.min(1, Math.max(0, part / whole)) : 0;
}

/** Normalizes a typed site the way native matches it: `https://www.X.com/a`
 *  becomes `x.com`. Null when it is not a site that can be blocked. */
export function normalizeSite(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed || trimmed.length > 2048) return null;
  try {
    const url = new URL(trimmed.includes("://") ? trimmed : `https://${trimmed}`);
    if (url.protocol !== "http:" && url.protocol !== "https:") return null;
    let host = url.hostname.replace(/\.$/u, "");
    if (host.startsWith("[") || /^\d+(\.\d+){3}$/u.test(host)) return null;
    if (host.startsWith("www.")) host = host.slice(4);
    return host.includes(".") && host.length <= 253 ? host : null;
  } catch {
    return null;
  }
}

import type { TimeBucketView } from "$shared/ipc/bindings";
import { dateOfDay, elapsedBuckets, type Period } from "$domain/time";

export interface Totals {
  browse: number;
  work: number;
  all: number;
}

export function totals(buckets: TimeBucketView[]): Totals {
  let browse = 0;
  let work = 0;
  for (const bucket of buckets) {
    browse += bucket.browse;
    work += bucket.work;
  }
  return { browse, work, all: browse + work };
}

/** Seconds per bucket, web and Work together. */
export function columns(buckets: TimeBucketView[]): number[] {
  return buckets.map((bucket) => bucket.browse + bucket.work);
}

/** How a period compares with the one before, in seconds: positive is more.
 *  A running day has nothing fair to compare with yet; a week compares its
 *  daily average so far with last week's, as a running week has only had
 *  some of its days. Null when there is nothing to say. */
export function comparison(
  period: Period,
  current: Totals,
  previous: TimeBucketView,
  now = new Date(),
): number | null {
  const before = previous.browse + previous.work;
  const elapsed = elapsedBuckets(period, now);
  if (period.span === "day") {
    const running = elapsed < 24;
    return running || before === 0 ? null : current.all - before;
  }
  if (before === 0 || elapsed === 0) return null;
  return current.all / elapsed - before / 7;
}

/** Below this a difference reads as "about the same". */
export const STEADY_SECONDS = 5 * 60;

/** Daily average over the days of the period that have begun. */
export function dailyAverage(period: Period, current: Totals, now = new Date()): number | null {
  if (period.span !== "week") return null;
  const elapsed = elapsedBuckets(period, now);
  return elapsed > 0 ? current.all / elapsed : null;
}

const hourFormat = new Intl.DateTimeFormat(undefined, { hour: "numeric" });

/** The bucket as the readout names it: "2 – 3 PM", "Tuesday, Sep 30". */
export function describeBucket(period: Period, index: number): string {
  if (period.span === "day") {
    const start = new Date(2000, 0, 1, index);
    // The last hour ends on the next day, which a range would spell out in
    // full dates; it is still just the hour before midnight.
    if (index >= 23)
      return `${hourFormat.format(start)} – ${hourFormat.format(new Date(2000, 0, 1, 0))}`;
    return hourFormat.formatRange(start, new Date(2000, 0, 1, index + 1));
  }
  return new Intl.DateTimeFormat(undefined, {
    weekday: "long",
    month: "short",
    day: "numeric",
  }).format(dateOfDay(period.start + index));
}

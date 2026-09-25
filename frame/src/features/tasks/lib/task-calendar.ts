/** Calendar arithmetic for the scheduling popover.
 *
 *  Every value here is a `YYYY-MM-DD` calendar day held in UTC. A due date has
 *  no time and no zone, so nothing in this file may cross local midnight or a
 *  daylight-saving boundary and land a day out.
 */
const DAY_MS = 86_400_000;
const MONTH_LABEL = new Intl.DateTimeFormat(undefined, {
  month: "long",
  year: "numeric",
  timeZone: "UTC",
});
const WEEKDAY_SHORT = new Intl.DateTimeFormat(undefined, { weekday: "short", timeZone: "UTC" });
const WEEKDAY_LONG = new Intl.DateTimeFormat(undefined, { weekday: "long", timeZone: "UTC" });
const DAY_NUMBER = new Intl.DateTimeFormat(undefined, { day: "numeric", timeZone: "UTC" });
const FULL_DAY = new Intl.DateTimeFormat(undefined, {
  weekday: "long",
  day: "numeric",
  month: "long",
  year: "numeric",
  timeZone: "UTC",
});

export function parseDay(day: string): Date | null {
  const parts = /^(\d{4})-(\d{2})-(\d{2})$/u.exec(day);
  if (!parts) return null;
  const at = new Date(Date.UTC(Number(parts[1]), Number(parts[2]) - 1, Number(parts[3])));
  // Rejects the shapes that parse but are not the day they claim, such as 02-31.
  return formatDay(at) === day ? at : null;
}

export function formatDay(at: Date): string {
  return `${at.getUTCFullYear()}-${String(at.getUTCMonth() + 1).padStart(2, "0")}-${String(at.getUTCDate()).padStart(2, "0")}`;
}

export function addDays(day: string, count: number): string {
  const at = parseDay(day);
  return at === null ? day : formatDay(new Date(at.getTime() + count * DAY_MS));
}

/** Monday in most of the world; the platform says so where it knows. */
export function weekStart(): number {
  const locale = new Intl.Locale(
    typeof navigator === "undefined" ? "en" : navigator.language || "en",
  ) as Intl.Locale & { getWeekInfo?: () => { firstDay: number }; weekInfo?: { firstDay: number } };
  const first = locale.getWeekInfo?.().firstDay ?? locale.weekInfo?.firstDay;
  return typeof first === "number" && first >= 1 && first <= 7 ? first % 7 : 1;
}

/** Days from `day` forward to the next occurrence of a weekday, never zero: a
 *  reader asking for "Saturday" on a Saturday means the next one. */
function forwardTo(day: string, weekday: number): number {
  const at = parseDay(day);
  if (at === null) return 0;
  return (weekday - at.getUTCDay() + 7) % 7 || 7;
}

export function nextWeekday(day: string, weekday: number): string {
  return addDays(day, forwardTo(day, weekday));
}

/** The start of the week after this one. */
export function nextWeek(day: string, startsOn = weekStart()): string {
  return addDays(day, forwardTo(day, startsOn));
}

export function monthOf(day: string): { year: number; month: number } {
  const at = parseDay(day) ?? new Date();
  return { year: at.getUTCFullYear(), month: at.getUTCMonth() };
}

export function shiftMonth(year: number, month: number, delta: number) {
  const at = new Date(Date.UTC(year, month + delta, 1));
  return { year: at.getUTCFullYear(), month: at.getUTCMonth() };
}

export function monthLabel(year: number, month: number): string {
  return MONTH_LABEL.format(new Date(Date.UTC(year, month, 1)));
}

export function dayNumber(day: string): string {
  const at = parseDay(day);
  return at === null ? "" : DAY_NUMBER.format(at);
}

export function dayName(day: string, style: "short" | "long" = "short"): string {
  const at = parseDay(day);
  if (at === null) return "";
  return (style === "short" ? WEEKDAY_SHORT : WEEKDAY_LONG).format(at);
}

export function fullDayName(day: string): string {
  const at = parseDay(day);
  return at === null ? day : FULL_DAY.format(at);
}

export function weekdayNames(startsOn = weekStart()): string[] {
  // 2026-02-01 is a Sunday, so offsetting from it walks the week in order.
  return Array.from({ length: 7 }, (_unused, index) =>
    WEEKDAY_SHORT.format(new Date(Date.UTC(2026, 1, 1 + ((startsOn + index) % 7)))),
  );
}

/** Six aligned weeks, so the popover never changes height between months. */
export function monthGrid(year: number, month: number, startsOn = weekStart()): string[][] {
  const first = new Date(Date.UTC(year, month, 1));
  const lead = (first.getUTCDay() - startsOn + 7) % 7;
  const origin = first.getTime() - lead * DAY_MS;
  return Array.from({ length: 6 }, (_week, week) =>
    Array.from({ length: 7 }, (_day, day) =>
      formatDay(new Date(origin + (week * 7 + day) * DAY_MS)),
    ),
  );
}

export function inMonth(day: string, year: number, month: number): boolean {
  const at = parseDay(day);
  return at !== null && at.getUTCFullYear() === year && at.getUTCMonth() === month;
}

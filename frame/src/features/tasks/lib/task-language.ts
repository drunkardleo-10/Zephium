/** Reading a date and a time out of what someone typed.
 *
 *  The rule that keeps this pleasant rather than infuriating: only a phrase at
 *  the very end of the line is ever consumed. That is how people write a task —
 *  "Call Anna tomorrow at 3" — and it means "Meet the Friday team" keeps its
 *  Friday, because the word is in the middle.
 *
 *  Nothing here guesses. A phrase either matches the vocabulary exactly, at a
 *  word boundary, or the line is left alone.
 */
import type { TaskPriority } from "$domain/resources";
import { addDays, nextWeek, nextWeekday, parseDay } from "./task-calendar";

export type Capture = {
  /** What the task should be called, with any consumed phrase removed. */
  title: string;
  dueDate: string | null;
  dueTime: string | null;
  /** The text that was understood, so the field can show its reading back. */
  matched: string | null;
};

const WEEKDAYS: Record<string, number> = {
  sunday: 0,
  sun: 0,
  monday: 1,
  mon: 1,
  tuesday: 2,
  tue: 2,
  tues: 2,
  wednesday: 3,
  wed: 3,
  weds: 3,
  thursday: 4,
  thu: 4,
  thur: 4,
  thurs: 4,
  friday: 5,
  fri: 5,
  saturday: 6,
  sat: 6,
};

const MONTHS: Record<string, number> = {
  january: 1,
  jan: 1,
  february: 2,
  feb: 2,
  march: 3,
  mar: 3,
  april: 4,
  apr: 4,
  may: 5,
  june: 6,
  jun: 6,
  july: 7,
  jul: 7,
  august: 8,
  aug: 8,
  september: 9,
  sept: 9,
  sep: 9,
  october: 10,
  oct: 10,
  november: 11,
  nov: 11,
  december: 12,
  dec: 12,
};

/** Connectors that only ever introduced the phrase just consumed. */
const CONNECTOR = /\s+(?:at|on|by|due|@)$/iu;

type Hit<T> = { value: T; rest: string; text: string };

function hit<T>(value: T, input: string, match: RegExpMatchArray): Hit<T> {
  const at = match.index ?? 0;
  return { value, rest: input.slice(0, at), text: input.slice(at).trim() };
}

function clock(hour: number, minute: number): string | null {
  if (hour > 23 || minute > 59) return null;
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
}

function matchTime(input: string): Hit<string> | null {
  const meridiem = input.match(/(?:^|\s)(\d{1,2})(?::(\d{2}))?\s*(am|pm)$/iu);
  if (meridiem) {
    const raw = Number(meridiem[1]);
    if (raw >= 1 && raw <= 12) {
      const pm = meridiem[3]!.toLowerCase() === "pm";
      const value = clock((raw % 12) + (pm ? 12 : 0), Number(meridiem[2] ?? 0));
      if (value) return hit(value, input, meridiem);
    }
  }
  const twentyFour = input.match(/(?:^|\s)(\d{1,2}):(\d{2})$/u);
  if (twentyFour) {
    const value = clock(Number(twentyFour[1]), Number(twentyFour[2]));
    if (value) return hit(value, input, twentyFour);
  }
  const named = input.match(/(?:^|\s)(noon|midday|midnight)$/iu);
  if (named) return hit(named[1]!.toLowerCase() === "midnight" ? "00:00" : "12:00", input, named);
  return null;
}

/** An evening word carries its own hour unless one was typed. */
const EVENING = "20:00";

type DateHit = Hit<string> & { impliedTime?: string };

function matchDate(input: string, today: string): DateHit | null {
  const plain = input.match(/(?:^|\s)(today|tonight|tomorrow|tmrw|tmr)$/iu);
  if (plain) {
    const word = plain[1]!.toLowerCase();
    const day = word === "today" || word === "tonight" ? today : addDays(today, 1);
    const found: DateHit = hit(day, input, plain);
    if (word === "tonight") found.impliedTime = EVENING;
    return found;
  }
  const week = input.match(/(?:^|\s)next\s+week$/iu);
  if (week) return hit(nextWeek(today), input, week);

  const weekend = input.match(/(?:^|\s)(?:this\s+|next\s+)?weekend$/iu);
  if (weekend) return hit(nextWeekday(today, 6), input, weekend);

  const named = input.match(
    /(?:^|\s)(?:next\s+)?(sunday|sun|monday|mon|tuesday|tues|tue|wednesday|weds|wed|thursday|thurs|thur|thu|friday|fri|saturday|sat)$/iu,
  );
  if (named) return hit(nextWeekday(today, WEEKDAYS[named[1]!.toLowerCase()]!), input, named);

  const relative = input.match(/(?:^|\s)in\s+(\d{1,3})\s+(day|days|week|weeks)$/iu);
  if (relative) {
    const count = Number(relative[1]);
    const days = relative[2]!.toLowerCase().startsWith("week") ? count * 7 : count;
    if (days >= 1 && days <= 3650) return hit(addDays(today, days), input, relative);
  }

  const iso = input.match(/(?:^|\s)(\d{4}-\d{2}-\d{2})$/u);
  if (iso && parseDay(iso[1]!)) return hit(iso[1]!, input, iso);

  // Whole month words only. A trailing `[a-z]*` would read "Check market 5" as
  // the third of March, which is precisely the kind of guess that makes a
  // parser like this hated.
  const dayFirst = input.match(
    /(?:^|\s)(\d{1,2})(?:st|nd|rd|th)?\s+(january|jan|february|feb|march|mar|april|apr|may|june|jun|july|jul|august|aug|september|sept|sep|october|oct|november|nov|december|dec)\.?$/iu,
  );
  const monthFirst = input.match(
    /(?:^|\s)(january|jan|february|feb|march|mar|april|apr|may|june|jun|july|jul|august|aug|september|sept|sep|october|oct|november|nov|december|dec)\.?\s+(\d{1,2})(?:st|nd|rd|th)?$/iu,
  );
  const calendar = dayFirst
    ? { day: Number(dayFirst[1]), month: MONTHS[dayFirst[2]!.toLowerCase()]!, match: dayFirst }
    : monthFirst
      ? {
          day: Number(monthFirst[2]),
          month: MONTHS[monthFirst[1]!.toLowerCase()]!,
          match: monthFirst,
        }
      : null;
  if (calendar) {
    const origin = parseDay(today);
    if (origin) {
      // A bare "3 Sep" means the next one: this year if it is still ahead, the
      // next otherwise. Nobody writes a task for a date that has passed.
      for (const year of [origin.getUTCFullYear(), origin.getUTCFullYear() + 1]) {
        const value = `${year}-${String(calendar.month).padStart(2, "0")}-${String(calendar.day).padStart(2, "0")}`;
        if (parseDay(value) && value >= today) return hit(value, input, calendar.match);
      }
    }
  }
  return null;
}

/** Reads the line, consuming at most one date and one time from its tail. */
export function parseCapture(input: string, today: string): Capture {
  const original = input.trim().replace(/\s+/gu, " ");
  let rest = original;
  let dueDate: string | null = null;
  let dueTime: string | null = null;
  let implied: string | null = null;

  // Two passes: either order reads, so "tomorrow at 3pm" and "at 3pm tomorrow"
  // both land.
  for (let pass = 0; pass < 2; pass += 1) {
    const trimmed = rest.replace(CONNECTOR, "");
    const clock: Hit<string> | null = dueTime === null ? matchTime(trimmed) : null;
    if (clock) {
      dueTime = clock.value;
      rest = clock.rest;
      continue;
    }
    const day: DateHit | null = dueDate === null ? matchDate(trimmed, today) : null;
    if (day) {
      dueDate = day.value;
      implied = day.impliedTime ?? implied;
      rest = day.rest;
      continue;
    }
    break;
  }

  const title = rest.replace(CONNECTOR, "").trim();
  // Consuming the whole line would leave a task with no name — or with only the
  // connector that introduced the phrase. That is a task called "tomorrow",
  // not a task due tomorrow.
  if (!title || /^(?:at|on|by|due|@)$/iu.test(title) || (dueDate === null && dueTime === null))
    return { title: original, dueDate: null, dueTime: null, matched: null };
  // A time needs a day to belong to, which is the contract's rule too.
  if (dueDate === null) dueDate = today;
  if (dueTime === null && implied !== null) dueTime = implied;
  return { title, dueDate, dueTime, matched: original.slice(title.length).trim() || null };
}

/** Reads a time on its own, for a field that asks only for one. */
export function parseTime(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;
  return matchTime(trimmed)?.value ?? null;
}

export type TokenKind = "priority" | "list" | "duration";
export type Tokens = {
  /** The line with every consumed token removed. */
  rest: string;
  priority: TaskPriority | null;
  list: string | null;
  duration: number | null;
  /** What was consumed, per kind, so the field can show and refuse each one. */
  matched: Partial<Record<TokenKind, string>>;
};

const PRIORITY: Record<string, TaskPriority> = {
  "1": "high",
  high: "high",
  "2": "medium",
  medium: "medium",
  med: "medium",
  "3": "low",
  low: "low",
};

const squash = (value: string) => value.toLocaleLowerCase().replace(/[\s_-]+/gu, "");

/** `#list`: an exact name first, then a prefix only one list starts with. */
function findList(word: string, lists: readonly TaskListLike[]): string | null {
  const wanted = squash(word);
  if (!wanted) return null;
  const exact = lists.find((list) => squash(list.title) === wanted);
  if (exact) return exact.id;
  const prefixed = lists.filter((list) => squash(list.title).startsWith(wanted));
  return prefixed.length === 1 ? prefixed[0]!.id : null;
}

function minutesOf(text: string): number | null {
  const parts = /^(?:(\d{1,3}(?:\.\d+)?)h)?(?:(\d{1,4})m(?:in)?)?$/iu.exec(text);
  if (!parts || (!parts[1] && !parts[2])) return null;
  const total = Math.round(Number(parts[1] ?? 0) * 60 + Number(parts[2] ?? 0));
  return total >= 1 && total <= 7 * 24 * 60 ? total : null;
}

type TaskListLike = { id: string; title: string };

/** Reads sigil tokens anywhere in the line: `!1`–`!3` (or `!high`), `#list` and
 *  `~45m`. A sigil is never ordinary prose, so position does not matter the way
 *  it does for dates. The last token of a kind wins; unknown ones stay as text. */
export function parseTokens(
  input: string,
  lists: readonly TaskListLike[] = [],
  skip: ReadonlySet<TokenKind> = new Set(),
): Tokens {
  const found: Tokens = { rest: input, priority: null, list: null, duration: null, matched: {} };
  const keep: string[] = [];
  for (const word of input.split(/(\s+)/u)) {
    const sigil = word[0];
    const body = word.slice(1);
    if (sigil === "!" && !skip.has("priority") && PRIORITY[body.toLowerCase()]) {
      found.priority = PRIORITY[body.toLowerCase()]!;
      found.matched.priority = word;
      continue;
    }
    if (sigil === "#" && !skip.has("list")) {
      const list = findList(body, lists);
      if (list) {
        found.list = list;
        found.matched.list = word;
        continue;
      }
    }
    if (sigil === "~" && !skip.has("duration")) {
      const minutes = minutesOf(body);
      if (minutes !== null) {
        found.duration = minutes;
        found.matched.duration = word;
        continue;
      }
    }
    keep.push(word);
  }
  found.rest = keep.join("").replace(/\s+/gu, " ").trim();
  return found;
}

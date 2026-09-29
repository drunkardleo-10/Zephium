import type { WorkEnvironmentSummary, WorkRuntimeProjection } from "$shared/ipc/bindings";
import * as m from "$shared/i18n/messages";

/** Names a work carries until something names it: today's and the earlier runtime's. */
const DEFAULTS = new Set(["untitled project", "new work"]);

/** Whether a work still carries the name it was made with. */
export function defaultTitle(title: string, untitled: string): boolean {
  const name = title.trim().toLowerCase();
  return !name || name === untitled.trim().toLowerCase() || DEFAULTS.has(name);
}

/** Words a short name never ends on. */
const TRAILING = new Set(
  "a an and as at by for from in into of on or the to with my our your me please".split(" "),
);
const NAME = { chars: 40, words: 7 } as const;

/**
 * A work's name from the person's first words, when nothing better names it:
 * its first line, whole words up to a short line's length, never ending on a
 * joining word, never an ellipsis.
 */
export function requestName(request: string): string {
  const line = request
    .split(/\n|(?<=[.?!])\s/u)[0]!
    .replace(/\s+/gu, " ")
    .trim();
  const words: string[] = [];
  let length = 0;
  for (const word of line.split(" ")) {
    if (words.length >= NAME.words || length + word.length > NAME.chars) break;
    words.push(word);
    length += word.length + 1;
  }
  if (!words.length && line) words.push(line.slice(0, NAME.chars));
  while (words.length > 2 && TRAILING.has(words.at(-1)!.toLowerCase().replace(/\W+$/u, "")))
    words.pop();
  const name = words.join(" ").replace(/[\s,;:.–—-]+$/u, "");
  return name.charAt(0).toLocaleUpperCase() + name.slice(1);
}

/** What a listed work goes by: its own name, else what its first run called it, else its first words. */
export function listedName(work: WorkEnvironmentSummary, untitled: string): string {
  if (!defaultTitle(work.title, untitled)) return work.title;
  return work.name || (work.requests[0] ? requestName(work.requests[0]) : "");
}

/**
 * The name a work takes from its first request once its first run has ended:
 * the first of its runs to give a title, else what the first run's reply was
 * called, else the request's first words.
 */
export function firstRunName(
  projection: WorkRuntimeProjection | undefined,
  request: string,
  live: boolean,
): string {
  const runs = projection?.executions ?? [];
  const first = runs[0];
  if (!first || live) return "";
  const titled = runs.map((run) => run.title?.trim() ?? "").find(Boolean);
  const reply = first.artifacts
    .find((artifact) => artifact.data.kind === "reply" || artifact.data.kind === "answer")
    ?.title.trim();
  return (titled || reply || requestName(request)).slice(0, 64);
}

const DAY = 24 * 60 * 60 * 1000;
let formats: {
  time: Intl.DateTimeFormat;
  weekday: Intl.DateTimeFormat;
  date: Intl.DateTimeFormat;
  year: Intl.DateTimeFormat;
} | null = null;
function f() {
  formats ??= {
    time: new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }),
    weekday: new Intl.DateTimeFormat(undefined, { weekday: "long" }),
    date: new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short" }),
    year: new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short", year: "numeric" }),
  };
  return formats;
}

/** When a work was last worked in, as its row says it: a time today, Yesterday, a weekday, a date. */
export function workedAt(time: number, now: number): string {
  const midnight = new Date(now);
  midnight.setHours(0, 0, 0, 0);
  const today = midnight.getTime();
  if (time >= today) return f().time.format(time);
  if (time >= today - DAY) return m.work_menu_yesterday();
  if (time >= today - 6 * DAY) return f().weekday.format(time);
  return new Date(time).getFullYear() === new Date(now).getFullYear()
    ? f().date.format(time)
    : f().year.format(time);
}

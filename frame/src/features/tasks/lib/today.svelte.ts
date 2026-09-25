import { todayKey } from "./task-sections";

/** The local calendar day, shared by every task surface.
 *
 *  One timer rather than one per list, so two surfaces can never disagree about
 *  when today ends, and a hidden panel is not paying for a second interval.
 */
let current = $state(todayKey());
let timer: ReturnType<typeof setInterval> | undefined;
let watchers = 0;

export function today(): string {
  return current;
}

/** Keeps the day current while a surface is showing it. */
export function watchToday(): () => void {
  watchers += 1;
  // Midnight while a list is open must not leave yesterday's work in Today.
  timer ??= setInterval(() => {
    const now = todayKey();
    if (now !== current) current = now;
  }, 60_000);
  return () => {
    watchers -= 1;
    if (watchers > 0 || timer === undefined) return;
    clearInterval(timer);
    timer = undefined;
  };
}

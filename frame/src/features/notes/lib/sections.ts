import type { NoteSummary } from "$domain/notes";

export type SectionKey =
  | { kind: "pinned" }
  | { kind: "today" }
  | { kind: "yesterday" }
  | { kind: "week" }
  | { kind: "month" }
  | { kind: "calendar"; year: number; month: number }
  | { kind: "results" };

export type Section = { key: SectionKey; id: string; items: NoteSummary[] };

const DAY = 24 * 60 * 60 * 1000;

function startOfDay(time: number): number {
  const date = new Date(time);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

function sectionOf(modified: number, today: number): SectionKey {
  if (modified >= today) return { kind: "today" };
  if (modified >= today - DAY) return { kind: "yesterday" };
  if (modified >= today - 7 * DAY) return { kind: "week" };
  if (modified >= today - 30 * DAY) return { kind: "month" };
  const date = new Date(modified);
  return { kind: "calendar", year: date.getFullYear(), month: date.getMonth() };
}

function idOf(key: SectionKey): string {
  return key.kind === "calendar" ? `${key.year}-${key.month}` : key.kind;
}

/** Notes grouped as they are read: pinned first, then by how recently they
 *  changed. A search result keeps its ranking and has no dates. */
export function sections(items: NoteSummary[], now: number, ranked: boolean): Section[] {
  if (ranked) return items.length ? [{ key: { kind: "results" }, id: "results", items }] : [];
  const today = startOfDay(now);
  const result: Section[] = [];
  const pinned = items.filter((item) => item.pinned);
  if (pinned.length) result.push({ key: { kind: "pinned" }, id: "pinned", items: pinned });
  for (const item of items) {
    if (item.pinned) continue;
    const key = sectionOf(Number(item.modified_at), today);
    const id = idOf(key);
    const last = result.at(-1);
    if (last?.id === id) last.items.push(item);
    else result.push({ key, id, items: [item] });
  }
  return result;
}

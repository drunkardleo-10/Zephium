import { formatValue } from "$shared/ui/data/Chart";
import type { SheetColumn, SheetView } from "../../lib/board/types";

type YesNo = "yes" | "no" | "partial" | "unknown";
export const yesNo = (text: string): YesNo => {
  const value = text.trim().toLowerCase();
  return value === "yes" || value === "no" || value === "partial" ? value : "unknown";
};

/** "4/5", "4.5 / 5", "9/10": a score and its scale. */
export function rating(text: string): { value: number; max: number } | null {
  const match = /^\s*(\d+(?:\.\d+)?)\s*\/\s*(\d+)\s*$/u.exec(text);
  if (!match) return null;
  const value = Number(match[1]);
  const max = Number(match[2]);
  return max > 0 && value >= 0 && value <= max ? { value, max } : null;
}

const decimal = (text: string) => {
  const value = Number(text.replace(/[,\s]/gu, ""));
  return text.trim() && Number.isFinite(value) ? value : null;
};

const NUMERIC = new Set<SheetColumn["kind"]>(["number", "money", "percent", "duration"]);
export const numeric = (column: SheetColumn) => NUMERIC.has(column.kind);

/** What a cell sorts and compares by: its number, its score, its yes; text as text. */
export function cellOrder(column: SheetColumn, text: string): number | string | null {
  switch (column.kind) {
    case "number":
    case "money":
    case "percent":
    case "duration":
      return decimal(text);
    case "rating": {
      const score = rating(text);
      return score ? score.value / score.max : null;
    }
    case "yes_no":
      return { yes: 2, partial: 1, no: 0, unknown: null }[yesNo(text)];
    default:
      return text.trim() ? text.trim() : null;
  }
}

/** A figure as a reader expects it: money in its currency, a percent with its sign. */
export function figure(column: SheetColumn, text: string): string {
  const value = decimal(text);
  if (value === null) return text;
  switch (column.kind) {
    case "money":
      return column.currency
        ? formatValue(value, { format: "money", currency: column.currency })
        : formatValue(value);
    case "percent":
      return formatValue(value, { format: "percent" });
    case "duration":
      return column.unit ? formatValue(value) : formatValue(value, { format: "duration" });
    default:
      return formatValue(value);
  }
}

/** The rows holding each column's best value, when the column asks and the values differ. */
export function bests(sheet: Pick<SheetView, "columns" | "rows">): Set<string>[] {
  return sheet.columns.map((column, index) => {
    if (!column.best) return new Set();
    const values = sheet.rows.map((row) => cellOrder(column, row.cells[index] ?? ""));
    const numbers = values.filter((value): value is number => typeof value === "number");
    if (numbers.length < 2) return new Set();
    const target = column.best === "max" ? Math.max(...numbers) : Math.min(...numbers);
    if (numbers.every((value) => value === target)) return new Set();
    return new Set(values.flatMap((value, row) => (value === target ? [String(row)] : [])));
  });
}

/** The host a link reads as: no scheme, no www. */
export function host(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./u, "");
  } catch {
    return url;
  }
}

/** A column that says something in some row: one empty in every row is left out. */
export function filled(sheet: Pick<SheetView, "columns" | "rows">, index: number): boolean {
  const column = sheet.columns[index];
  return sheet.rows.some((row) => {
    const cell = (row.cells[index] ?? "").trim();
    return !!cell && !(column?.kind === "yes_no" && yesNo(cell) === "unknown");
  });
}

/** The room a typed column takes at reading size, padding included. */
const ROOM: Partial<Record<SheetColumn["kind"], number>> = {
  money: 104,
  number: 88,
  percent: 80,
  duration: 96,
  date: 120,
  yes_no: 72,
  rating: 104,
  tag: 128,
  link: 160,
};
/** Kinds that say the most in the least room, first; text follows, shortest first. */
const TELLING: SheetColumn["kind"][] = [
  "money",
  "yes_no",
  "rating",
  "number",
  "percent",
  "duration",
  "tag",
  "date",
  "entity",
  "link",
];
/** A character of body text, a cell's padding, the widest a text column grows before it wraps. */
const CHAR = 7;
const PAD = 24;
const MEASURE = 240;

/**
 * The columns a sheet shows in the width it has, in their own order: its
 * subject always, then the typed columns that say most in least room, then
 * text, shortest first. What does not fit waits behind "more columns".
 */
export function fitColumns(sheet: Pick<SheetView, "columns" | "rows">, width: number): number[] {
  // A heading wraps between its words: a column needs its longest word, not its whole name.
  const word = (index: number) =>
    Math.max(0, ...(sheet.columns[index]?.label ?? "").split(/\s+/u).map((part) => part.length));
  const longest = (index: number) =>
    Math.max(word(index), ...sheet.rows.map((row) => (row.cells[index] ?? "").trim().length));
  const room = (index: number) => {
    const fixed = ROOM[sheet.columns[index]!.kind];
    if (fixed) return Math.max(fixed, word(index) * CHAR + PAD);
    // Text wraps: it needs its longest word and a fair measure, not its whole length.
    return Math.min(MEASURE, Math.max(96, Math.ceil(longest(index) * 0.55) * CHAR)) + PAD;
  };
  const rank = (index: number) => {
    const kind = sheet.columns[index]!.kind;
    return TELLING.includes(kind) ? TELLING.indexOf(kind) : TELLING.length + longest(index) / 100;
  };
  const candidates = sheet.columns
    .map((_, index) => index)
    .slice(1)
    .filter((index) => filled(sheet, index))
    .sort((a, b) => rank(a) - rank(b) || a - b);
  let left = width - Math.min(220, longest(0) * CHAR + 40);
  const kept: number[] = [];
  for (const index of candidates) {
    const need = room(index);
    if (need > left) continue;
    kept.push(index);
    left -= need;
  }
  return kept.sort((a, b) => a - b);
}

/** The subject that is best on most of the measures the sheet marks best, if one is. */
export function pickOf(sheet: Pick<SheetView, "columns" | "rows">): number | null {
  const counts = sheet.rows.map(() => 0);
  for (const best of bests(sheet)) for (const row of best) counts[Number(row)]! += 1;
  const top = Math.max(0, ...counts);
  if (!top || counts.filter((count) => count === top).length > 1) return null;
  return counts.indexOf(top);
}

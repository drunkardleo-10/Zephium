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

/** The room a column takes when read from afar, at the overview's type size. */
const FAR: Partial<Record<SheetColumn["kind"], number>> = {
  money: 150,
  number: 120,
  percent: 110,
  duration: 130,
  date: 160,
  yes_no: 76,
  rating: 140,
  tag: 190,
  link: 220,
};
/** Kinds that say the most in the least room, first. */
const TELLING: SheetColumn["kind"][] = [
  "money",
  "yes_no",
  "rating",
  "tag",
  "number",
  "percent",
  "duration",
  "date",
  "entity",
];
/** An overview character at 22 px, and the widest a subject may stand before it wraps. */
const CHAR = 11;
const SUBJECT = 200;
const GAP = 24;

/**
 * The columns a sheet keeps from afar, in their own order: the typed ones that
 * say most first, then short text; a column of sentences gives way. They fit
 * the room beside the subject column.
 */
export function farColumns(sheet: Pick<SheetView, "columns" | "rows">, width: number): number[] {
  const longest = (index: number) =>
    Math.max(0, ...sheet.rows.map((row) => (row.cells[index] ?? "").trim().length));
  const average = (index: number) =>
    sheet.rows.reduce((sum, row) => sum + (row.cells[index] ?? "").trim().length, 0) /
    Math.max(1, sheet.rows.length);
  const room = (index: number) => {
    const column = sheet.columns[index]!;
    const fixed = FAR[column.kind];
    if (fixed) return fixed;
    // Short text may wrap to two lines from afar; its label still wants its own line.
    const words = Math.max(column.label.length, Math.ceil(longest(index) * 0.6));
    return Math.min(260, words * CHAR + (column.kind === "entity" ? 40 : 0));
  };
  const short = (index: number) =>
    sheet.columns[index]!.kind !== "text" || (average(index) <= 32 && longest(index) <= 48);
  const candidates = sheet.columns
    .map((column, index) => ({ column, index }))
    .filter(({ index }) => index > 0 && short(index))
    .sort((a, b) => {
      const rank = (kind: SheetColumn["kind"]) =>
        TELLING.includes(kind) ? TELLING.indexOf(kind) : TELLING.length;
      return rank(a.column.kind) - rank(b.column.kind) || a.index - b.index;
    });
  let left =
    width -
    Math.min(SUBJECT, Math.max(sheet.columns[0]?.label.length ?? 0, longest(0)) * CHAR + 40) -
    56;
  const kept: number[] = [];
  for (const { index } of candidates) {
    const need = room(index) + GAP;
    if (need > left) continue;
    kept.push(index);
    left -= need;
  }
  return kept.sort((a, b) => a - b);
}

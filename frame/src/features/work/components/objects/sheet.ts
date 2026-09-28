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

import type { ColumnType, TableColumn, TimelineStop } from "./types";

const FIGURE =
  /^[~≈+\-−]?\s?(?:[$€£¥]|[A-Z]{3}\s)?\s?\d[\d,.\s]*(?:%|[kmb]n?|x|\/\w+)?(?:\s?(?:[–-]|to)\s?(?:[$€£¥])?\d[\d,.\s]*(?:%|[kmb]n?)?)?(?:\s?\/\s?\w+)?$/iu;
const CURRENCY = /[$€£¥]|\b(?:USD|EUR|GBP|PLN|JPY)\b/u;
const PRICE = /\b(price|cost|fee|fare|rent|budget|salary|pay|spend)\b/iu;
const LINK = /^https?:\/\/\S+$/iu;
const MONTH =
  /\b(jan(uary)?|feb(ruary)?|mar(ch)?|apr(il)?|may|june?|july?|aug(ust)?|sep(t(ember)?)?|oct(ober)?|nov(ember)?|dec(ember)?)\b/iu;
const ISO = /^\d{4}-\d{2}(-\d{2})?(T[\d:]+)?/u;
/** An ordered stop an itinerary or a roadmap names: a date, a day, a week, a quarter. */
const STOP = /^(day|week|month|phase|stage|q[1-4]|h[12])\s*\d*\b/iu;

const dated = (cell: string) => {
  const value = cell.trim();
  return ISO.test(value) || (MONTH.test(value) && /\d/u.test(value) && value.length <= 32);
};
const stop = (cell: string) => dated(cell) || STOP.test(cell.trim());

/** How each column reads, from its header and every filled cell. */
export function columnTypes(columns: readonly string[], rows: readonly (readonly string[])[]) {
  return columns.map((label, index): TableColumn => {
    const cells = rows.map((row) => row[index]?.trim() ?? "").filter(Boolean);
    let type: ColumnType = "text";
    if (cells.length && cells.every((cell) => LINK.test(cell))) type = "link";
    else if (cells.length && cells.every((cell) => FIGURE.test(cell)))
      type = PRICE.test(label) || cells.every((cell) => CURRENCY.test(cell)) ? "money" : "number";
    else if (cells.length && cells.every(dated)) type = "date";
    else {
      const mean = cells.reduce((sum, cell) => sum + cell.length, 0) / Math.max(1, cells.length);
      if (mean > 36) type = "long";
    }
    return { label, type };
  });
}

/**
 * A table whose first column is a run of dated or numbered stops reads as a
 * timeline: each row a stop, its first text column the title, the rest detail.
 */
export function timelineOf(
  columns: readonly string[],
  rows: readonly (readonly string[])[],
): TimelineStop[] | null {
  if (rows.length < 3 || columns.length < 2) return null;
  if (!rows.every((row) => stop(row[0] ?? ""))) return null;
  return rows.map((row) => {
    const [when = "", title = "", ...rest] = row.map((cell) => cell.trim());
    const detail = rest.filter(Boolean).join(" · ");
    return { when, title, ...(detail ? { detail } : {}) };
  });
}

/** A category label as a table's first column spells it, for matching a chart to its table. */
export const labelKey = (text: string) =>
  text
    .normalize("NFKC")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();

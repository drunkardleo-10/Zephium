import type { ArtifactContent } from "./artifact";

type Tabular = Extract<ArtifactContent, { kind: "table" | "comparison" }>;

/** What a table card shows before it counts the rest. */
export const TABLE_CARD = { rows: 6, columns: 4 } as const;

/** A figure as a source writes it: a sign, a currency, digits, a unit or a range of them. */
const FIGURE =
  /^[+\-−]?[$€£¥]?\s?\d[\d,.\s]*(?:%|[kmb]n?|x)?(?:\s?[–-]\s?[$€£¥]?\d[\d,.\s]*(?:%|[kmb]n?)?)?$/iu;
const figure = (cell: string) => FIGURE.test(cell.trim());

/** One grid for a table or a plain comparison: an alternative's name is its first column. */
export function tableGrid(content: Tabular): { columns: string[]; rows: string[][] } {
  if (content.kind === "table")
    return { columns: [...content.columns], rows: content.rows.map((row) => [...row]) };
  return {
    columns: ["", ...content.criteria],
    rows: content.alternatives.map((row) => [row.name, ...row.values]),
  };
}

/**
 * How each column reads: figures align right on tabular numerals; the one
 * column whose cells run longest (a "why") takes 40% of a wide table.
 */
export function tableColumns(grid: { columns: string[]; rows: string[][] }): {
  numeric: boolean[];
  long: number;
} {
  const numeric = grid.columns.map((_, index) => {
    const cells = grid.rows.map((row) => row[index]?.trim() ?? "").filter(Boolean);
    return cells.length > 0 && cells.every(figure);
  });
  let long = -1;
  let longest = 40;
  grid.columns.forEach((_, index) => {
    if (numeric[index]) return;
    const cells = grid.rows.map((row) => row[index] ?? "");
    const mean = cells.reduce((sum, cell) => sum + cell.length, 0) / Math.max(1, cells.length);
    if (mean > longest) [long, longest] = [index, mean];
  });
  return { numeric, long };
}

/** A cell's leading number: "$1,299.00", "4.5/5", "12k", "3–5" (its first figure). */
function figureValue(cell: string): number | null {
  const match =
    /^([+\-−])?\s*(?:[$€£¥]|[A-Z]{3}\s)?\s*(\d[\d,]*(?:\.\d+)?)(?:\s?([kKmMbB])(?![\p{L}]))?(?![\d.,])/u.exec(
      cell.trim(),
    );
  if (!match) return null;
  const scale = { k: 1e3, m: 1e6, b: 1e9 }[match[3]?.toLowerCase() ?? ""] ?? 1;
  const value = Number(match[2]!.replace(/,/gu, "")) * scale;
  if (!Number.isFinite(value)) return null;
  return match[1] && match[1] !== "+" ? -value : value;
}

const filled = (grid: { rows: string[][] }, column: number) =>
  grid.rows.map((row) => row[column]?.trim() ?? "").filter(Boolean);

/** Whether every filled cell of a column is a number, so it sorts as one. */
function sortsAsNumber(grid: { rows: string[][] }, column: number): boolean {
  const cells = filled(grid, column);
  return cells.length > 0 && cells.every((cell) => figureValue(cell) !== null);
}

export type TableSort = { column: number; descending: boolean };

/** Row order for a sort: numbers and money by value, text by locale; empty cells last. */
export function sortedRows(grid: { rows: string[][] }, sort: TableSort | null): number[] {
  const order = grid.rows.map((_, index) => index);
  if (!sort) return order;
  const numeric = sortsAsNumber(grid, sort.column);
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });
  const cell = (index: number) => grid.rows[index]?.[sort.column]?.trim() ?? "";
  return order.sort((a, b) => {
    const left = cell(a);
    const right = cell(b);
    if (!left || !right) return left ? -1 : right ? 1 : a - b;
    const by = numeric ? figureValue(left)! - figureValue(right)! : collator.compare(left, right);
    return (sort.descending ? -by : by) || a - b;
  });
}

const PRICE = /\b(price|cost|fee|msrp|rrp|fare|rent)\b/iu;
const RATING = /\b(rating|score|stars?)\b/iu;
const CURRENCY = /[$€£¥]|\b[A-Z]{3}\b/u;

/**
 * The best row of each money or rating column, as the compare card marks it:
 * the lowest price, the highest rating, only where the known values differ.
 */
export function bestCells(grid: { columns: string[]; rows: string[][] }): Map<number, number[]> {
  const best = new Map<number, number[]>();
  grid.columns.forEach((name, column) => {
    if (column === 0) return;
    const cells = grid.rows.map((row) => row[column]?.trim() ?? "");
    const known = cells.filter(Boolean);
    if (known.length < 2 || !known.every((cell) => figureValue(cell) !== null)) return;
    const money = PRICE.test(name) || known.every((cell) => CURRENCY.test(cell));
    const way = money ? "low" : RATING.test(name) ? "high" : null;
    if (!way) return;
    if (money && new Set(known.map((cell) => cell.match(CURRENCY)?.[0] ?? "")).size > 1) return;
    const values = cells.map((cell) => (cell ? figureValue(cell) : null));
    const numbers = values.filter((value): value is number => value !== null);
    const target = way === "low" ? Math.min(...numbers) : Math.max(...numbers);
    if (numbers.every((value) => value === target)) return;
    best.set(
      column,
      values.flatMap((value, row) => (value === target ? [row] : [])),
    );
  });
  return best;
}

/** The table as CSV text: every field quoted when it must be. */
export function tableCsv(columns: readonly string[], rows: readonly (readonly string[])[]): string {
  const field = (value: string) =>
    /[",\n\r]/u.test(value) ? `"${value.replaceAll('"', '""')}"` : value;
  return [columns, ...rows].map((row) => row.map(field).join(",")).join("\r\n");
}

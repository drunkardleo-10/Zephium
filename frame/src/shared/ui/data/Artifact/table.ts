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

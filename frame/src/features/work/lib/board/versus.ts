import type { SheetView } from "./types";

/**
 * A sheet that compares a few named things: its subjects stand as columns,
 * each under its mark, and what they are compared on reads down as rows.
 */
export function versus(sheet: Pick<SheetView, "columns" | "rows">): boolean {
  if (sheet.columns[0]?.kind !== "entity") return false;
  if (sheet.rows.length < 2 || sheet.rows.length > 5) return false;
  const measures = sheet.columns.filter(
    (_, index) => index > 0 && sheet.rows.some((row) => (row.cells[index] ?? "").trim()),
  );
  return measures.length >= 2 && measures.length <= 10;
}

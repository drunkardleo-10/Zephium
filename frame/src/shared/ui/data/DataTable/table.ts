/** Presentation-only values. These are not Work artifact or IPC schemas. */
export type TableColumn = { key: string; label: string; numeric?: boolean };
export type TableRow = {
  key: string;
  label: string;
  cells: Readonly<Record<string, string | null>>;
};
export type TableLabels = {
  rowHeading: string;
  actions: string;
  empty: string;
  missing: string;
  unavailable: string;
  previous: string;
  next: string;
  range: (first: number, last: number, total: number) => string;
};

export const MAX_TABLE_ROWS = 10_000;
export const MAX_TABLE_COLUMNS = 32;
export function tableInputValid(
  rows: readonly TableRow[],
  columns: readonly TableColumn[],
): boolean {
  return (
    rows.length <= MAX_TABLE_ROWS &&
    columns.length <= MAX_TABLE_COLUMNS &&
    new Set(rows.map((row) => row.key)).size === rows.length &&
    new Set(columns.map((column) => column.key)).size === columns.length
  );
}

/** Presentation-only values. These are not Work artifact or IPC schemas. */
export type TableColumn = {
  key: string;
  label: string;
  numeric?: boolean;
  /** Marks (checks, dots) stand centred under their heading. */
  centered?: boolean;
  /** The header sorts the rows by this column's `sort` values. */
  sortable?: boolean;
};
export type TableRow = {
  key: string;
  label: string;
  cells: Readonly<Record<string, string | null>>;
  /** What the row sorts by in each sortable column; the row label sorts the heading. */
  sort?: Readonly<Record<string, number | string | null>>;
};
export type TableSort = { key: string; descending: boolean };

/** Rows in a column's order; a row with no value sorts last either way. */
export function sortRows(rows: readonly TableRow[], sort: TableSort | null): readonly TableRow[] {
  if (!sort) return rows;
  const value = (row: TableRow) =>
    sort.key === "" ? row.label : (row.sort?.[sort.key] ?? row.cells[sort.key] ?? null);
  return [...rows].sort((a, b) => {
    const x = value(a);
    const y = value(b);
    if (x === null || y === null) return x === y ? 0 : x === null ? 1 : -1;
    const order =
      typeof x === "number" && typeof y === "number"
        ? x - y
        : String(x).localeCompare(String(y), undefined, { numeric: true });
    return sort.descending ? -order : order;
  });
}
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

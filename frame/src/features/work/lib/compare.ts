import type {
  CellView,
  CriterionView,
  EvidenceReference,
  SubjectView,
} from "$shared/ui/data/Artifact";
import { formatMoney, subjectKey } from "./subjects";

export type ComparePicture = { profile: string; digest: string };
type CompareColumn = {
  key: string;
  name: string;
  descriptor?: string;
  homepage?: string;
  picture?: ComparePicture;
  /** What a person looks at first: the price, taken out of the rows. */
  price?: string;
  /** The lowest price among the columns, when prices differ. */
  best?: boolean;
};
type CompareValue =
  | { kind: "text"; text: string }
  | { kind: "number"; text: string }
  | { kind: "mark"; yes: boolean }
  | { kind: "unknown" };
export type CompareCell = {
  value: CompareValue;
  note?: string;
  evidence: readonly EvidenceReference[];
  generalKnowledge: boolean;
  /** Where the cell sits in the artifact, so a correction reaches it. */
  subject: number;
  criterion: number;
  /** In a row of numbers, this value as a share of the row's largest: a bar. */
  share?: number;
  /** The best value of its row, where the criterion says which way is better. */
  best?: boolean;
};
type CompareRow = {
  key: string;
  label: string;
  meta?: string;
  numeric: boolean;
  /** Every known value is a yes or a no: the row reads as marks, never as bars. */
  check: boolean;
  cells: readonly CompareCell[];
};
export type CompareModel = {
  columns: readonly CompareColumn[];
  rows: readonly CompareRow[];
  notes: readonly string[];
};
type Matrix = {
  subjects: readonly SubjectView[];
  criteria: readonly CriterionView[];
  cells: readonly (readonly CellView[])[];
  notes: readonly string[];
};

const YES = new Set([
  "yes",
  "y",
  "true",
  "included",
  "include",
  "standard",
  "supported",
  "available",
  "in stock",
  "✓",
]);
const NO = new Set([
  "no",
  "n",
  "false",
  "not included",
  "excluded",
  "none",
  "unsupported",
  "unavailable",
  "out of stock",
  "✗",
  "-",
  "—",
]);
const PRICE = /^(price|cost|msrp|rrp|list price|retail price|price \(usd\)|starting price)$/u;

function plain(text: string): string {
  return text.normalize("NFKC").trim().toLowerCase().replace(/\s+/gu, " ").replace(/\.$/u, "");
}
/** A criterion name as a row label: "piece_count" and "Piece Count" read alike. */
function label(name: string): string {
  const words = name.replace(/[_-]+/gu, " ").replace(/\s+/gu, " ").trim();
  return words.charAt(0).toLocaleUpperCase() + words.slice(1);
}
function measurementMeta(criterion: CriterionView): string | undefined {
  if (criterion.kind === "measurement") return criterion.basis || undefined;
  if (criterion.kind === "rating") return criterion.rubric || undefined;
  return undefined;
}
/** The number a measured cell holds, when it holds exactly one. */
function magnitude(cell: CellView | undefined): number | null {
  if (!cell) return null;
  const raw =
    cell.value.kind === "money"
      ? cell.value.amount
      : cell.value.kind === "measurement"
        ? cell.value.value
        : cell.value.kind === "rating"
          ? String(cell.value.value)
          : null;
  if (raw === null || !/^\s*[\d.,]+\s*$/u.test(raw)) return null;
  const number = Number(raw.replace(/,/gu, ""));
  return Number.isFinite(number) && number >= 0 ? number : null;
}
/** Bars only when every known value is a comparable number and two differ. */
function shares(cells: readonly (CellView | undefined)[]): (number | undefined)[] | null {
  const currencies = new Set(
    cells.flatMap((cell) => (cell?.value.kind === "money" ? [cell.value.currency] : [])),
  );
  if (currencies.size > 1) return null;
  const values = cells.map(magnitude);
  const known = values.filter((entry): entry is number => entry !== null);
  const max = Math.max(...known);
  if (known.length < 2 || known.length !== cells.filter(Boolean).length || max <= 0) return null;
  if (known.every((entry) => entry === known[0])) return null;
  return values.map((entry) => (entry === null ? undefined : entry / max));
}
/**
 * Which way is better, only where the criterion says so: a price or cost is
 * better lower, a rating higher. A plain measurement has no direction.
 */
function direction(criterion: CriterionView | undefined, cells: readonly (CellView | undefined)[]) {
  if (!criterion) return null;
  if (criterion.kind === "rating") return "high" as const;
  if (PRICE.test(plain(criterion.name)) || cells.some((cell) => cell?.value.kind === "money"))
    return "low" as const;
  return null;
}
/** Where the best value stands: every column holding it, when the known values differ. */
function bestOf(
  criterion: CriterionView | undefined,
  cells: readonly (CellView | undefined)[],
): Set<number> {
  const way = direction(criterion, cells);
  const currencies = new Set(
    cells.flatMap((cell) => (cell?.value.kind === "money" ? [cell.value.currency] : [])),
  );
  if (!way || currencies.size > 1) return new Set();
  const values = cells.map(magnitude);
  const known = values.filter((entry): entry is number => entry !== null);
  if (known.length < 2 || known.every((entry) => entry === known[0])) return new Set();
  const target = way === "low" ? Math.min(...known) : Math.max(...known);
  return new Set(values.flatMap((entry, index) => (entry === target ? [index] : [])));
}
function value(cell: CellView | undefined, criterion: CriterionView): CompareValue {
  if (!cell || cell.value.kind === "unknown") return { kind: "unknown" };
  switch (cell.value.kind) {
    case "money":
      return { kind: "number", text: formatMoney(cell.value.amount, cell.value.currency) };
    case "measurement":
      return {
        kind: "number",
        text:
          criterion.kind === "measurement" && criterion.unit
            ? `${cell.value.value} ${criterion.unit}`
            : cell.value.value,
      };
    case "rating":
      return { kind: "number", text: `${cell.value.value}/${criterion.scaleMax ?? 5}` };
    case "presence":
      return { kind: "mark", yes: cell.value.present };
    default: {
      const text = cell.value.text.trim();
      return text ? { kind: "text", text } : { kind: "unknown" };
    }
  }
}
/** A column of yes/no answers reads as glyphs; one "None" among prose does not. */
function marked(values: readonly CompareValue[]): CompareValue[] | null {
  const known = values.filter((entry) => entry.kind !== "unknown");
  if (
    !known.length ||
    !known.every((entry) => entry.kind === "text" && boolean(entry.text) !== null)
  )
    return null;
  return values.map((entry) =>
    entry.kind === "text" ? { kind: "mark", yes: boolean(entry.text)! } : entry,
  );
}
function boolean(text: string): boolean | null {
  const word = plain(text);
  return YES.has(word) ? true : NO.has(word) ? false : null;
}
/** The criterion a person reads as the price, if the run recorded one. */
function priceColumn(matrix: Matrix): number {
  const named = matrix.criteria.findIndex((criterion) => PRICE.test(plain(criterion.name)));
  if (named >= 0) return named;
  return matrix.criteria.findIndex((_, column) =>
    matrix.cells.some((row) => row[column]?.value.kind === "money"),
  );
}

/** A comparison read as a product compare: one column per subject, price first. */
export function compareModel(
  matrix: Matrix,
  pictures: ReadonlyMap<string, ComparePicture> = new Map(),
): CompareModel {
  const price = priceColumn(matrix);
  const cheapest =
    price >= 0
      ? bestOf(
          matrix.criteria[price],
          matrix.subjects.map((_, index) => matrix.cells[index]?.[price]),
        )
      : new Set<number>();
  const columns = matrix.subjects.map((subject, index) => {
    const cell = price >= 0 ? matrix.cells[index]?.[price] : undefined;
    const criterion = price >= 0 ? matrix.criteria[price] : undefined;
    const money = cell && criterion ? value(cell, criterion) : undefined;
    return {
      key: `${index}:${subject.name}`,
      name: subject.name,
      ...(subject.descriptor ? { descriptor: subject.descriptor } : {}),
      ...(subject.homepage ? { homepage: subject.homepage } : {}),
      ...(pictures.get(subjectKey(subject)) ? { picture: pictures.get(subjectKey(subject))! } : {}),
      ...(money && money.kind !== "unknown" && money.kind !== "mark" ? { price: money.text } : {}),
      ...(cheapest.has(index) ? { best: true } : {}),
    };
  });
  const rows = matrix.criteria.flatMap((criterion, column) => {
    if (column === price) return [];
    const raw = matrix.subjects.map((_, index) => value(matrix.cells[index]?.[column], criterion));
    const values = marked(raw) ?? raw;
    const meta = measurementMeta(criterion);
    const row = matrix.subjects.map((_, index) => matrix.cells[index]?.[column]);
    const check =
      values.some((entry) => entry.kind === "mark") &&
      values.every((entry) => entry.kind === "mark" || entry.kind === "unknown");
    const bars = check ? null : shares(row);
    const best = check ? new Set<number>() : bestOf(criterion, row);
    return [
      {
        key: `${column}:${criterion.name}`,
        label: label(criterion.name),
        ...(meta ? { meta } : {}),
        numeric: values.every((entry) => entry.kind === "number" || entry.kind === "unknown"),
        check,
        cells: matrix.subjects.map((_, index) => {
          const cell = matrix.cells[index]?.[column];
          return {
            value: values[index] ?? { kind: "unknown" as const },
            ...(cell?.note ? { note: cell.note } : {}),
            evidence: cell?.evidence ?? [],
            generalKnowledge: !!cell?.generalKnowledge,
            subject: index,
            criterion: column,
            ...(bars?.[index] !== undefined ? { share: bars[index] } : {}),
            ...(best.has(index) ? { best: true } : {}),
          };
        }),
      },
    ];
  });
  return { columns, rows, notes: matrix.notes };
}

/** The text a correction starts from: what the cell says now. */
export function cellText(value: CompareCell["value"], yes: string, no: string): string {
  switch (value.kind) {
    case "text":
    case "number":
      return value.text;
    case "mark":
      return value.yes ? yes : no;
    default:
      return "";
  }
}

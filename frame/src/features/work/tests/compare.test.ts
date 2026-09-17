import { expect, test } from "vitest";
import { compareModel } from "../lib/compare";
import type { CellView, CriterionView, SubjectView } from "$shared/ui/data/Artifact";

const cell = (value: CellView["value"], evidence: CellView["evidence"] = []): CellView => ({
  value,
  evidence,
  generalKnowledge: false,
});
const subjects: SubjectView[] = [{ name: "Tower Bridge" }, { name: "Eiffel Tower" }];
const criteria: CriterionView[] = [
  { name: "Piece_count", kind: "measurement", unit: "pieces" },
  { name: "Price", kind: "text" },
  { name: "Minifigures included", kind: "text" },
  { name: "Notes", kind: "text" },
];
const cells: CellView[][] = [
  [
    cell({ kind: "measurement", value: "4295" }),
    cell({ kind: "money", amount: "239.99", currency: "USD" }),
    cell({ kind: "text", text: "Yes" }),
    cell({ kind: "text", text: "None" }),
  ],
  [
    cell({ kind: "unknown" }),
    cell({ kind: "money", amount: "629.99", currency: "USD" }),
    cell({ kind: "text", text: "No" }),
    cell({ kind: "text", text: "Ships flat" }),
  ],
];

test("price leads the column and leaves the rows", () => {
  const model = compareModel({ subjects, criteria, cells, notes: [] });
  expect(model.columns.map((column) => [column.name, column.price])).toEqual([
    ["Tower Bridge", "$239.99"],
    ["Eiffel Tower", "$629.99"],
  ]);
  expect(model.rows.map((row) => row.label)).toEqual([
    "Piece count",
    "Minifigures included",
    "Notes",
  ]);
});

test("a yes/no column becomes glyphs; prose with a 'None' in it does not", () => {
  const model = compareModel({ subjects, criteria, cells, notes: [] });
  const marks = model.rows.find((row) => row.label === "Minifigures included")!;
  expect(marks.cells.map((entry) => entry.value)).toEqual([
    { kind: "mark", yes: true },
    { kind: "mark", yes: false },
  ]);
  const prose = model.rows.find((row) => row.label === "Notes")!;
  expect(prose.cells.map((entry) => entry.value.kind)).toEqual(["text", "text"]);
});

test("measurements carry their unit, unknown cells carry nothing", () => {
  const model = compareModel({ subjects, criteria, cells, notes: [] });
  const row = model.rows[0]!;
  expect(row.numeric).toBe(true);
  expect(row.cells.map((entry) => entry.value)).toEqual([
    { kind: "number", text: "4295 pieces" },
    { kind: "unknown" },
  ]);
});

test("a subject's admitted picture reaches its column", () => {
  const model = compareModel(
    { subjects, criteria, cells, notes: [] },
    new Map([["name:tower bridge", { profile: "profile", digest: "digest" }]]),
  );
  expect(model.columns[0]?.picture).toEqual({ profile: "profile", digest: "digest" });
  expect(model.columns[1]?.picture).toBeUndefined();
});

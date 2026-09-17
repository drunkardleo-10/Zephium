import { expect, test } from "vitest";
import type { WorkExecutionFact } from "$shared/ipc/bindings";
import { subjectFacts, subjectImageCandidates, subjectKey } from "../lib/subjects";
import { projection } from "./environment-fixtures";

type Artifacts = WorkExecutionFact["artifacts"];

function run(): WorkExecutionFact {
  return structuredClone(projection.executions[0]!);
}

test("one name is one subject whatever page the run attached to it", () => {
  const product = "https://www.lego.com/en-us/product/tower-bridge-21067";
  const theme = "https://www.lego.com/en-us/themes/architecture";
  const key = subjectKey({ name: "Tower Bridge" });
  expect(subjectKey({ name: "Tower Bridge", homepage: product })).toBe(key);
  expect(subjectKey({ name: "Tower Bridge", homepage: theme })).toBe(key);
  expect(subjectKey({ name: " Tower  Bridge. " })).toBe(key);
  expect(subjectKey({ name: "TOWER BRIDGE" })).toBe(key);
  // Two things that merely share a catalogue page stay two things.
  expect(subjectKey({ name: "Paris", homepage: theme })).not.toBe(
    subjectKey({ name: "New York City", homepage: theme }),
  );
  // Dash and quote variants are the same name.
  expect(subjectKey({ name: "Eiffel – Tower’s" })).toBe(subjectKey({ name: "Eiffel - Tower's" }));
});

test("a merged subject keeps the facts of every artifact that described it", () => {
  const execution = run();
  const matrix = (subject: object, criteria: object[], cells: object[]) => ({
    ...execution.artifacts[0]!,
    id: JSON.stringify(cells),
    data: { kind: "comparison_matrix", subjects: [subject], criteria, cells: [cells], notes: [] },
  });
  execution.artifacts = [
    matrix(
      { name: "Tower Bridge", homepage: "https://shop.example/p/1?utm_medium=a" },
      [
        { name: "price", kind: { kind: "text" } },
        { name: "product url", kind: { kind: "text" } },
      ],
      [
        { value: { kind: "text", text: "$119.99" }, evidence: [], general_knowledge: false },
        {
          value: { kind: "text", text: "https://shop.example/p/1" },
          evidence: [],
          general_knowledge: false,
        },
      ],
    ),
    matrix(
      { name: "Tower Bridge" },
      [
        { name: "Price", kind: { kind: "text" } },
        { name: "pieces", kind: { kind: "measurement", unit: "pieces", basis: "listed" } },
      ],
      [
        {
          value: { kind: "money", amount: "119.99", currency: "USD" },
          evidence: [],
          general_knowledge: false,
        },
        { value: { kind: "measurement", value: "4295" }, evidence: [], general_knowledge: false },
      ],
    ),
  ] as Artifacts;
  // The price cell wins over the earlier text row and the label never repeats.
  expect(subjectFacts(execution, { name: "tower bridge" })).toEqual([
    { label: "price", value: "$119.99" },
    { label: "pieces", value: "4295 pieces" },
  ]);
  expect(subjectFacts(execution, { name: "Paris" })).toEqual([]);
});

test("a fact never restates the subject's own identity", () => {
  const execution = run();
  const text = (value: string) => ({
    value: { kind: "text", text: value },
    evidence: [],
    general_knowledge: false,
  });
  execution.artifacts = [
    {
      ...execution.artifacts[0]!,
      id: "final",
      data: {
        kind: "comparison_matrix",
        subjects: [{ name: "Tower Bridge" }],
        criteria: [
          { name: "Title", kind: { kind: "text" } },
          { name: "Product URL", kind: { kind: "text" } },
          { name: "image_url", kind: { kind: "text" } },
          { name: "Set", kind: { kind: "text" } },
          { name: "Price", kind: { kind: "text" } },
          { name: "Piece Count", kind: { kind: "text" } },
        ],
        cells: [
          [
            text("Tower Bridge"),
            text("shop.example/p/1"),
            text("img.example/a.jpg"),
            text("tower  bridge."),
            text("$119.99"),
            text("4295"),
          ],
        ],
        notes: [],
      },
    },
  ] as Artifacts;
  // Title, the two link columns, and the column that merely repeats the name
  // are identity, not facts.
  expect(subjectFacts(execution, { name: "Tower Bridge" })).toEqual([
    { label: "Price", value: "$119.99" },
    { label: "Piece Count", value: "4295" },
  ]);
});

test("a detail read outranks the catalogue row for the same criterion", () => {
  const execution = run();
  const text = (value: string) => ({
    value: { kind: "text", text: value },
    evidence: [],
    general_knowledge: false,
  });
  const matrix = (id: string, subjects: object[], cells: object[][]) => ({
    ...execution.artifacts[0]!,
    id,
    title: id,
    data: {
      kind: "comparison_matrix",
      subjects,
      criteria: [
        { name: "product url", kind: { kind: "text" } },
        { name: "displayed price", kind: { kind: "text" } },
        { name: "piece count", kind: { kind: "text" } },
      ],
      cells,
      notes: [],
    },
  });
  execution.artifacts = [
    // The catalogue lists nine sets in listing prose, and one cell is only the
    // set's own name dressed up with a trademark mark.
    matrix(
      "catalog",
      [{ name: "New York City" }, { name: "Himeji Castle" }],
      [
        [text("https://shop.example/p/1"), text("$349.99 New"), text("LEGO® New York City!")],
        [text("https://shop.example/p/2"), text("$199.99 New"), text("2125 pieces")],
      ],
    ),
    // The product page read is one row, and it read the real values.
    matrix("detail", [{ name: "New York City" }], [[text(""), text("$349.99"), text("3745")]]),
  ] as Artifacts;
  execution.steps = [
    {
      id: "read-1",
      turn: 1,
      kind: { kind: "read", url: "https://shop.example/catalog", collection: null },
      status: "succeeded",
      artifacts: ["catalog"],
    },
    {
      id: "read-2",
      turn: 2,
      kind: { kind: "read", url: "https://shop.example/p/1", collection: null },
      status: "succeeded",
      artifacts: ["detail"],
    },
  ] as WorkExecutionFact["steps"];
  expect(subjectFacts(execution, { name: "New York City" })).toEqual([
    { label: "displayed price", value: "$349.99" },
    { label: "piece count", value: "3745" },
  ]);
  // A set no detail read covered still keeps the catalogue's answer.
  expect(subjectFacts(execution, { name: "Himeji Castle" })).toEqual([
    { label: "displayed price", value: "$199.99 New" },
    { label: "piece count", value: "2125 pieces" },
  ]);
});

test("pictures a browser step recorded outrank the ones a table claims", () => {
  const execution = run();
  const artifact = (id: string, subjects: object[], kind: string) => ({
    ...execution.artifacts[0]!,
    id,
    data:
      kind === "comparison_matrix"
        ? { kind, subjects, criteria: [], cells: subjects.map(() => []), notes: [] }
        : { kind, subjects, summary: "read", entries: [] },
  });
  execution.artifacts = [
    artifact(
      "final",
      [
        { name: "Paris", image_candidates: ["https://img.example/tower-bridge.jpg"] },
        { name: "Tower Bridge", image_candidates: ["https://img.example/tower-bridge.jpg"] },
      ],
      "comparison_matrix",
    ),
    artifact(
      "detail",
      [{ name: "Paris", image_candidates: ["https://img.example/paris.jpg"] }],
      "evidence_collection",
    ),
    artifact(
      "catalog",
      [{ name: "Tower Bridge", image_candidates: ["https://img.example/tb-catalog.jpg"] }],
      "comparison_matrix",
    ),
  ] as Artifacts;
  execution.steps = [
    {
      id: "read-1",
      turn: 1,
      kind: { kind: "read", url: "https://shop.example/catalog", collection: null },
      status: "succeeded",
      artifacts: ["catalog"],
    },
  ] as WorkExecutionFact["steps"];
  expect(subjectImageCandidates(execution, { name: "Paris" })).toEqual([
    "https://img.example/paris.jpg",
    "https://img.example/tower-bridge.jpg",
  ]);
  expect(subjectImageCandidates(execution, { name: "Tower Bridge" })).toEqual([
    "https://img.example/tb-catalog.jpg",
    "https://img.example/tower-bridge.jpg",
  ]);
});

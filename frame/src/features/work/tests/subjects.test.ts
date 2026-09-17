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

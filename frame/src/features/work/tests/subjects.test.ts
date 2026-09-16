import { expect, test } from "vitest";
import type { WorkExecutionFact } from "$shared/ipc/bindings";
import { subjectFacts, subjectImageCandidates, subjectKey } from "../lib/subjects";
import { projection } from "./environment-fixtures";

test("subjects merge by page and gather facts and pictures across records", () => {
  const execution: WorkExecutionFact = structuredClone(projection.executions[0]!);
  const matrix = (subject: object, criteria: object[], cells: object[]) => ({
    ...execution.artifacts[0]!,
    id: JSON.stringify(cells),
    data: { kind: "comparison_matrix", subjects: [subject], criteria, cells: [cells], notes: [] },
  });
  execution.artifacts = [
    matrix(
      {
        name: "Tower Bridge",
        homepage: "https://shop.example/p/1?utm_medium=a",
        image_candidates: ["https://img.example/a.jpg"],
      },
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
      {
        name: "LEGO Tower Bridge",
        homepage: "https://shop.example/p/1",
        image_candidates: ["https://img.example/b.jpg"],
      },
      [
        { name: "price", kind: { kind: "text" } },
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
  ] as WorkExecutionFact["artifacts"];
  const subject = { name: "Other name", homepage: "https://shop.example/p/1/" };
  expect(subjectKey(subject)).toBe("page:shop.example/p/1");
  expect(subjectFacts(execution, subject)).toEqual([
    { label: "price", value: "$119.99" },
    { label: "pieces", value: "4295 pieces" },
  ]);
  expect(subjectImageCandidates(execution, subject)).toEqual([
    "https://img.example/a.jpg",
    "https://img.example/b.jpg",
  ]);
  expect(subjectKey({ name: " Bare " })).toBe("name:bare");
});

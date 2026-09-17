import { expect, test } from "vitest";
import type { WorkExecutionFact, WorkEnvironmentSnapshot } from "$shared/ipc/bindings";
import { organizeExecution, pendingOrganize } from "../lib/organize";
import { projection, snapshot as savedSnapshot } from "./environment-fixtures";

const snapshot: WorkEnvironmentSnapshot = {
  ...savedSnapshot,
  elements: savedSnapshot.elements.filter((element) => element.reference.kind === "objective"),
  relations: [],
  view: { ...savedSnapshot.view, placements: [] },
};

function agentRun(): WorkExecutionFact {
  const base = structuredClone(projection.executions[0]!);
  return {
    ...base,
    status: "running",
    authorization: "user_directed_agent",
    attempts: [{ id: "attempt", node: "node", status: "running", usage: null }],
    spec: {
      ...base.spec,
      nodes: [
        {
          ...base.spec.nodes[0]!,
          capability: {
            kind: "agent",
            grant: {
              provider: "open_ai",
              model: "gpt-5.6-luna",
              max_turns: 10,
              max_steps: 32,
              browse_hops: 4,
            },
          },
        },
      ],
    },
    provider_evidence: [
      {
        id: "record",
        node: "node",
        attempt: "attempt",
        evidence: {
          version: 1,
          provider: "open_ai",
          model: "gpt-5.6-luna",
          response_model: "gpt-5.6-luna",
          response_id: "resp_1",
          search_call_id: "ws_1",
          answer: "Two keyboards stand out",
          citations: [
            { url: "https://a.example/review", title: "Review A", start_index: 0, end_index: 3 },
            { url: "https://b.example/spec", title: "Spec B", start_index: 4, end_index: 7 },
          ],
          actual_input_tokens: 10,
          actual_output_tokens: 5,
        },
      },
    ],
    artifacts: [
      {
        ...base.artifacts[0]!,
        id: "sources",
        title: "Sources",
        data: {
          kind: "evidence_collection",
          summary: "Two keyboards stand out",
          subjects: [],
          entries: [
            { evidence: 0, title: "Review A", role: "source", subject: null },
            { evidence: 1, title: "Spec B", role: "source", subject: null },
          ],
        },
        evidence: [
          { extraction_id: "record", source_id: 1 },
          { extraction_id: "record", source_id: 2 },
        ],
      },
    ],
    steps: [
      {
        id: "search",
        turn: 1,
        kind: { kind: "search", query: "quiet keyboards" },
        status: "succeeded",
        usage: { model_tokens: 15, cost_micro_usd: 1, operations: 1, accounting: "exact" },
        artifacts: ["sources"],
        evidence: "record",
      },
    ],
  };
}

test("an agent run lands its sources while running and later findings connect to them", () => {
  const execution = agentRun();
  const state = { ...projection, executions: [execution] };
  expect(pendingOrganize(snapshot, state)?.id).toBe("execution");
  const first = organizeExecution(state, execution, { x: 100, y: 300 }, snapshot);
  expect(first.adds.map((add) => add.reference.kind)).toEqual(["source", "source"]);
  expect(first.adds[1]!.placement.y).toBeGreaterThan(first.adds[0]!.placement.y);
  expect(first.adds[1]!.placement.x).toBe(first.adds[0]!.placement.x);
  expect(first.relations).toEqual(
    first.adds.map((add) => ({
      from: { kind: "objective", objective: "objective" },
      to: add.reference,
      kind: "uses",
    })),
  );
  const placed: WorkEnvironmentSnapshot = {
    ...snapshot,
    elements: [
      ...snapshot.elements,
      ...first.adds.map((add, index) => ({
        id: `source-${index}`,
        area: null,
        reference: add.reference,
      })),
    ],
    view: {
      ...snapshot.view,
      placements: first.adds.map((add, index) => ({
        element: `source-${index}`,
        ...add.placement,
      })),
    },
  };
  expect(pendingOrganize(placed, state)).toBeNull();
  execution.artifacts.push({
    ...execution.artifacts[0]!,
    id: "findings",
    title: "Keyboards",
    data: {
      kind: "findings",
      subjects: [{ name: "Keyboard A" }],
      items: [
        { claim: "Quiet switches", subject: 0, evidence: [0], confidence: "supported" },
        { claim: "Ships in a week", subject: 0, evidence: [1], confidence: "supported" },
      ],
    },
    evidence: [
      { extraction_id: "record", source_id: 1 },
      { extraction_id: "record", source_id: 2 },
    ],
  });
  execution.steps!.push({
    id: "publish",
    turn: 2,
    kind: { kind: "publish" },
    status: "succeeded",
    artifacts: ["findings"],
  });
  expect(pendingOrganize(placed, state)?.id).toBe("execution");
  const second = organizeExecution(state, execution, { x: 100, y: 300 }, placed);
  expect(second.adds.map((add) => add.reference.kind)).toEqual(["subject", "finding", "finding"]);
  expect(second.relations).toEqual([
    { from: second.adds[1]!.reference, to: second.adds[0]!.reference, kind: "supports" },
    { from: first.adds[0]!.reference, to: second.adds[1]!.reference, kind: "supports" },
    { from: second.adds[2]!.reference, to: second.adds[0]!.reference, kind: "supports" },
    { from: first.adds[1]!.reference, to: second.adds[2]!.reference, kind: "supports" },
  ]);
  expect(second.adds[1]!.placement.y).toBeLessThan(second.adds[2]!.placement.y);
});

test("reviewed plans still organize once, after they settle", () => {
  expect(pendingOrganize(snapshot, projection)?.id).toBe("execution");
  expect(
    pendingOrganize(snapshot, {
      ...projection,
      executions: [{ ...projection.executions[0]!, status: "running" }],
    }),
  ).toBeNull();
  const plan = organizeExecution(projection, projection.executions[0]!, { x: 0, y: 0 }, snapshot);
  expect(plan.adds.map((add) => add.reference.kind)).toEqual(["artifact"]);
});

test("browser records merge into subject hubs by name and never place their tables", () => {
  const execution = agentRun();
  const theme = "https://shop.example/themes/architecture";
  const record = (id: string, subjects: { name: string; homepage?: string }[]) => ({
    ...execution.artifacts[0]!,
    id,
    title: id,
    data: {
      kind: "comparison_matrix" as const,
      subjects,
      criteria: [{ name: "price", kind: { kind: "text" as const } }],
      cells: subjects.map(() => [
        {
          value: { kind: "text" as const, text: "$119.99" },
          evidence: [0],
          note: null,
          general_knowledge: false,
        },
      ]),
      notes: [],
    },
    evidence: [{ extraction_id: "record", source_id: 1 }],
  });
  // The catalogue read names each set with its own product page.
  execution.artifacts = [
    record("catalog", [
      { name: "Tower Bridge", homepage: "https://shop.example/p/1?utm_source=x" },
      { name: "Paris", homepage: "https://shop.example/p/2" },
    ]),
  ];
  execution.steps = [
    {
      id: "read-1",
      turn: 1,
      kind: { kind: "read", url: "https://shop.example/catalog", collection: null },
      status: "succeeded",
      artifacts: ["catalog"],
    },
  ];
  const state = { ...projection, executions: [execution] };
  const first = organizeExecution(state, execution, { x: 100, y: 300 }, snapshot);
  expect(first.adds.map((add) => add.reference.kind)).toEqual(["subject", "subject"]);
  const placed: WorkEnvironmentSnapshot = {
    ...snapshot,
    elements: [
      ...snapshot.elements,
      ...first.adds.map((add, index) => ({
        id: `hub-${index}`,
        area: null,
        reference: add.reference,
      })),
    ],
    view: {
      ...snapshot.view,
      placements: first.adds.map((add, index) => ({ element: `hub-${index}`, ...add.placement })),
    },
  };
  expect(pendingOrganize(placed, state)).toBeNull();
  // The product page read repeats one set by name alone: still one hub.
  execution.artifacts.push(record("detail", [{ name: "Tower Bridge" }]));
  execution.steps.push({
    id: "read-2",
    turn: 2,
    kind: { kind: "read", url: "https://shop.example/p/1", collection: null },
    status: "succeeded",
    artifacts: ["detail"],
  });
  expect(pendingOrganize(placed, state)).toBeNull();
  // The published table gives every set the theme page; the names still hold.
  execution.artifacts.push({
    ...record("final", [
      { name: "Tower Bridge", homepage: theme },
      { name: "Paris", homepage: theme },
    ]),
    title: "Compared sets",
  });
  execution.steps.push({
    id: "publish",
    turn: 3,
    kind: { kind: "publish" },
    status: "succeeded",
    artifacts: ["final"],
  });
  expect(pendingOrganize(placed, state)?.id).toBe("execution");
  const second = organizeExecution(state, execution, { x: 100, y: 300 }, placed);
  expect(second.adds.map((add) => add.reference.kind)).toEqual(["artifact"]);
  expect(second.relations).toEqual([
    { from: first.adds[0]!.reference, to: second.adds[0]!.reference, kind: "uses" },
    { from: first.adds[1]!.reference, to: second.adds[0]!.reference, kind: "uses" },
  ]);
});

test("subjects that only share a homepage stay separate hubs", () => {
  const execution = agentRun();
  const theme = "https://shop.example/themes/architecture";
  execution.artifacts = [
    {
      ...execution.artifacts[0]!,
      id: "final",
      title: "Compared sets",
      data: {
        kind: "comparison_matrix",
        subjects: [
          { name: "Tower Bridge", homepage: theme },
          { name: "Paris", homepage: theme },
          { name: "New York City", homepage: theme },
        ],
        criteria: [],
        cells: [[], [], []],
        notes: [],
      },
      evidence: [],
    },
  ];
  execution.steps = [
    {
      id: "publish",
      turn: 1,
      kind: { kind: "publish" },
      status: "succeeded",
      artifacts: ["final"],
    },
  ];
  const plan = organizeExecution(
    { ...projection, executions: [execution] },
    execution,
    { x: 100, y: 300 },
    snapshot,
  );
  expect(plan.adds.map((add) => add.reference.kind)).toEqual([
    "subject",
    "subject",
    "subject",
    "artifact",
  ]);
});

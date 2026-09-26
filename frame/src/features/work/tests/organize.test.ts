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

test("an agent run leaves its cited pages to the Sources card and places what they establish", () => {
  const execution = agentRun();
  const state = { ...projection, executions: [execution] };
  // A search alone establishes nothing: its pages are rows on one Sources card.
  expect(pendingOrganize(snapshot, state)).toBeNull();
  expect(organizeExecution(state, execution, { x: 100, y: 300 }, snapshot).adds).toEqual([]);
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
  expect(pendingOrganize(snapshot, state)?.id).toBe("execution");
  const plan = organizeExecution(state, execution, { x: 100, y: 300 }, snapshot);
  // The findings artifact is one card; the subject it names is still a hub.
  expect(plan.adds.map((add) => add.reference.kind)).toEqual(["subject", "artifact"]);
  expect(plan.relations).toEqual([
    { from: plan.adds[1]!.reference, to: plan.adds[0]!.reference, kind: "supports" },
  ]);
  // The board places them; the saved placement only records the request's corner.
  expect(plan.adds.map((add) => [add.placement.x, add.placement.y])).toEqual([
    [100, 300],
    [100, 300],
  ]);
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
  // A catalogue read lists twenty sets with their product pages. Listing rows
  // establish nothing: not one of them earns a card.
  execution.artifacts = [
    record("catalog", [
      { name: "Tower Bridge", homepage: "https://shop.example/p/1?utm_source=x" },
      { name: "Paris", homepage: "https://shop.example/p/2" },
      { name: "London", homepage: "https://shop.example/p/3" },
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
  expect(pendingOrganize(snapshot, state)).toBeNull();
  expect(organizeExecution(state, execution, { x: 100, y: 300 }, snapshot).adds).toEqual([]);
  // The product page read is one row: that set is what the run is about.
  execution.artifacts.push(record("detail", [{ name: "Tower Bridge" }]));
  execution.steps.push({
    id: "read-2",
    turn: 2,
    kind: { kind: "read", url: "https://shop.example/p/1", collection: null },
    status: "succeeded",
    artifacts: ["detail"],
  });
  expect(pendingOrganize(snapshot, state)?.id).toBe("execution");
  const first = organizeExecution(state, execution, { x: 100, y: 300 }, snapshot);
  expect(first.adds.map((add) => add.reference.kind)).toEqual(["subject"]);
  const placed: WorkEnvironmentSnapshot = {
    ...snapshot,
    elements: [
      ...snapshot.elements,
      { id: "hub-0", area: null, reference: first.adds[0]!.reference },
    ],
    view: { ...snapshot.view, placements: [{ element: "hub-0", ...first.adds[0]!.placement }] },
  };
  expect(pendingOrganize(placed, state)).toBeNull();
  // The published table gives every set the theme page; the names still hold,
  // and the set it compares that no detail read covered earns its hub here.
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
  expect(second.adds.map((add) => add.reference.kind)).toEqual(["subject", "artifact"]);
  // London was only ever a catalogue row, so it never reaches the canvas.
  expect(second.relations).toEqual([
    { from: first.adds[0]!.reference, to: second.adds[1]!.reference, kind: "uses" },
    { from: second.adds[0]!.reference, to: second.adds[1]!.reference, kind: "uses" },
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

test("a comparison of two concepts from what the agent knows stands alone in Made", () => {
  const execution = agentRun();
  const text = (value: string) => ({ value: { kind: "text" as const, text: value } });
  execution.artifacts = [
    {
      ...execution.artifacts[0]!,
      id: "compare",
      title: "Rust memory and garbage collection",
      general_knowledge: true,
      data: {
        kind: "comparison_matrix",
        subjects: [{ name: "Rust ownership" }, { name: "Garbage collection" }],
        criteria: [{ name: "When memory is freed", kind: { kind: "text" } }],
        cells: [[text("At scope end")], [text("When the collector runs")]],
        notes: [],
      },
      evidence: [],
    },
    {
      ...execution.artifacts[0]!,
      id: "points",
      title: "What differs",
      general_knowledge: true,
      data: {
        kind: "findings",
        subjects: [{ name: "Rust ownership" }],
        items: [{ claim: "No pauses", subject: 0, evidence: [], confidence: "supported" }],
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
      artifacts: ["compare", "points"],
    },
  ];
  const state = {
    ...projection,
    work: { ...projection.work, objective: "Compare Rust memory with garbage collection" },
    executions: [execution],
  };
  const plan = organizeExecution(state, execution, { x: 100, y: 300 }, snapshot);
  // No subject cards and no ties to them: the table and the findings, nothing else new.
  expect(plan.adds.map((add) => add.reference)).toEqual([
    expect.objectContaining({ kind: "artifact", artifact: "compare" }),
    expect.objectContaining({ kind: "artifact", artifact: "points" }),
  ]);
  expect(plan.relations).toEqual([]);
});

test("a full canvas plans no more than it can hold and stops asking", () => {
  const execution = agentRun();
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
  });
  execution.steps!.push({
    id: "publish",
    turn: 2,
    kind: { kind: "publish" },
    status: "succeeded",
    artifacts: ["findings"],
  });
  const state = { ...projection, executions: [execution] };
  const filler = (count: number): WorkEnvironmentSnapshot => ({
    ...snapshot,
    elements: [
      ...snapshot.elements,
      ...Array.from({ length: count }, (_, index) => ({
        id: `filler-${index}`,
        reference: { kind: "resource" as const, resource: `resource-${index}` },
        area: null,
      })),
    ],
  });
  const nearlyFull = filler(500 - snapshot.elements.length - 2);
  expect(organizeExecution(state, execution, { x: 0, y: 0 }, nearlyFull).adds).toHaveLength(2);
  const full = filler(500 - snapshot.elements.length);
  expect(pendingOrganize(full, state)).toBeNull();
  expect(organizeExecution(state, execution, { x: 0, y: 0 }, full).adds).toEqual([]);
});

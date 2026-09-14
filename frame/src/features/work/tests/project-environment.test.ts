import { expect, test } from "vitest";
import { projection, snapshot } from "./environment-fixtures";
import { environmentItems } from "../lib/project-environment";
test("joins exact objective and historical artifact identities without replacing originals", () => {
  const items = environmentItems(snapshot, [], [], new Map([["objective", projection]]));
  expect(items[0]?.title).toBe("Investigate dependencies");
  expect(items[1]?.artifact?.content).toEqual({
    kind: "document",
    paragraphs: ["Reviewed findings"],
  });
  expect(projection.executions[0]?.artifacts[0]?.data).toEqual({
    kind: "document",
    paragraphs: ["Original evidence"],
  });
  const wrongExecution = {
    ...snapshot,
    elements: [
      {
        ...snapshot.elements[1]!,
        reference: {
          kind: "artifact" as const,
          objective: "objective",
          execution: "other",
          artifact: "artifact",
        },
      },
    ],
  };
  expect(
    environmentItems(wrongExecution, [], [], new Map([["objective", projection]]))[0]?.artifact,
  ).toBeUndefined();
});

test("objective status follows the latest durable execution and native interruption truth", () => {
  const completed = projection.executions[0]!;
  const failed = { ...completed, id: "latest", status: "failed" as const };
  const items = (state: typeof projection) =>
    environmentItems(snapshot, [], [], new Map([["objective", state]]));
  expect(items({ ...projection, executions: [completed, failed] })[0]?.detail).toBe(
    "Status: Failed",
  );
  expect(
    items({
      ...projection,
      executions: [{ ...failed, status: "running" }],
      interrupted: ["latest"],
    })[0]?.detail,
  ).toBe("Status: Interrupted");
  expect(items({ ...projection, executions: [] })[0]?.detail).toBe("Status: Plan ready");
});

test("a subject takes its picture from a uses relation to an admitted image", async () => {
  const { environmentItems } = await import("../lib/project-environment");
  const snapshot = {
    version: 1,
    id: "env",
    profile: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
    space: "s",
    title: "T",
    lifecycle: "active",
    revision: "3",
    elements: [
      {
        id: "e-subject",
        area: null,
        reference: { kind: "subject", objective: "w", execution: "x", artifact: "a", index: 0 },
      },
      { id: "e-media", area: null, reference: { kind: "resource", resource: "res-media" } },
      { id: "e-note", area: null, reference: { kind: "resource", resource: "res-note" } },
    ],
    areas: [],
    relations: [
      { id: "r1", from: "e-subject", to: "e-media", kind: "uses", origin: { kind: "user" } },
    ],
    view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
  } as unknown as Parameters<typeof environmentItems>[0];
  const media = new Map([
    [
      "res-media",
      {
        version: 1,
        kind: "image" as const,
        mime: "image/png",
        bytes: 10,
        digest: "d".repeat(64),
        name: "kb.png",
        origin: { kind: "imported" as const },
        width: 4,
        height: 4,
      },
    ],
  ]);
  const items = environmentItems(snapshot, [], [], new Map(), media);
  const subject = items.find((item) => item.id === "e-subject")!;
  expect(subject.image).toEqual({ profile: "01ARZ3NDEKTSV4RRFFQ69G5FAV", digest: "d".repeat(64) });
  const picture = items.find((item) => item.id === "e-media")!;
  expect(picture.type).toBe("media");
  expect(picture.media?.asset.name).toBe("kb.png");
  const note = items.find((item) => item.id === "e-note")!;
  expect(note.type).toBe("note");
  expect(note.image).toBeUndefined();
});

test("source elements read their citation and the running agent links to them", async () => {
  const { environmentAgents } = await import("../lib/project-environment");
  const state = structuredClone(projection);
  const execution = state.executions[0]!;
  execution.status = "running";
  execution.authorization = "user_directed_agent";
  execution.spec.nodes[0]!.capability = {
    kind: "agent",
    grant: {
      provider: "open_ai",
      model: "gpt-5.6-luna",
      max_turns: 10,
      max_steps: 32,
      browse_hops: 4,
    },
  };
  execution.provider_evidence = [
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
        answer: "One review",
        citations: [
          { url: "https://a.example/review", title: "Review A", start_index: 0, end_index: 3 },
        ],
        actual_input_tokens: 10,
        actual_output_tokens: 5,
      },
    },
  ];
  execution.artifacts = [
    {
      ...execution.artifacts[0]!,
      id: "sources",
      data: {
        kind: "evidence_collection",
        summary: "One review",
        subjects: [],
        entries: [{ evidence: 0, title: "Review A", role: "source", subject: null }],
      },
      evidence: [{ extraction_id: "record", source_id: 1 }],
    },
  ];
  execution.steps = [
    {
      id: "turn",
      turn: 1,
      kind: { kind: "turn" },
      status: "succeeded",
      usage: { model_tokens: 1, cost_micro_usd: 1, operations: 1, accounting: "exact" },
      note: "Reading the review.",
    },
    {
      id: "search",
      turn: 1,
      kind: { kind: "search", query: "keyboards" },
      status: "succeeded",
      usage: { model_tokens: 1, cost_micro_usd: 1, operations: 1, accounting: "exact" },
      artifacts: ["sources"],
      evidence: "record",
    },
    {
      id: "read",
      turn: 2,
      kind: { kind: "read", url: "https://a.example/review" },
      status: "running",
    },
  ];
  const objectiveElement = snapshot.elements.find(
    (element) => element.reference.kind === "objective",
  )!;
  const withSource = {
    ...snapshot,
    elements: [
      objectiveElement,
      {
        id: "source-1",
        area: null,
        reference: {
          kind: "source" as const,
          objective: "objective",
          execution: "execution",
          artifact: "sources",
          index: 0,
        },
      },
    ],
    view: {
      ...snapshot.view,
      placements: [
        { element: objectiveElement.id, x: 0, y: 0, width: 320, height: 150 },
        { element: "source-1", x: 400, y: 0, width: 260, height: 84 },
      ],
    },
  };
  const objectives = new Map([["objective", state]]);
  const items = environmentItems(withSource, [], [], objectives);
  const source = items.find((item) => item.type === "source")!;
  expect(source.title).toBe("Review A");
  expect(source.detail).toBe("a.example");
  expect(source.source).toEqual({ url: "https://a.example/review", role: "source" });
  // The agent stands beside the source it reads, tied to it by a working link.
  const agents = environmentAgents(withSource, objectives, () => "reading");
  expect(agents.items.map((item) => item.agent?.worker ?? false)).toEqual([false]);
  expect(agents.items[0]?.agent?.line).toBe("Reading the review.");
  expect(agents.links).toEqual([
    {
      id: "working:source-1",
      source: `agent:${objectiveElement.id}`,
      target: "source-1",
      kind: "working",
    },
  ]);
  expect(agents.positions[`agent:${objectiveElement.id}`]).toEqual({ x: 700, y: -8 });
});

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

test("the request card carries the person's sentence and never a status line", () => {
  const completed = projection.executions[0]!;
  const failed = { ...completed, id: "latest", status: "failed" as const };
  const items = (state: typeof projection) =>
    environmentItems(snapshot, [], [], new Map([["objective", state]]));
  for (const state of [
    { ...projection, executions: [completed, failed] },
    {
      ...projection,
      executions: [{ ...failed, status: "running" as const }],
      interrupted: ["latest"],
    },
    { ...projection, executions: [] },
  ]) {
    const request = items(state)[0]!;
    expect(request.title).toBe("Investigate dependencies");
    expect(request.detail).toBe("");
    expect(request.kind).toBe("Request");
  }
});

test("a subject takes its picture from a uses relation and the image is not a card", async () => {
  const { environmentItems, environmentLinks } = await import("../lib/project-environment");
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
  // The admitted picture belongs to the subject; it is not a card of its own.
  expect(items.map((item) => item.id)).toEqual(["e-subject", "e-note"]);
  expect(environmentLinks(snapshot)).toEqual([]);
  const note = items.find((item) => item.id === "e-note")!;
  expect(note.type).toBe("note");
  expect(note.image).toBeUndefined();
  // A media resource nothing admitted for a subject still stands on its own.
  const loose = {
    ...snapshot,
    relations: [],
  } as unknown as Parameters<typeof environmentItems>[0];
  const picture = environmentItems(loose, [], [], new Map(), media).find(
    (item) => item.id === "e-media",
  )!;
  expect(picture.type).toBe("media");
  expect(picture.media?.asset.name).toBe("kb.png");
});

test("catalogue rows enrich the hub a detail read created without becoming cards", async () => {
  const state = structuredClone(projection);
  const execution = state.executions[0]!;
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
  const matrix = (id: string, subjects: object[], price: string[]) => ({
    ...execution.artifacts[0]!,
    id,
    title: id,
    data: {
      kind: "comparison_matrix" as const,
      subjects,
      criteria: [{ name: "price", kind: { kind: "text" as const } }],
      cells: price.map((text) => [
        { value: { kind: "text" as const, text }, evidence: [], general_knowledge: false },
      ]),
      notes: [],
    },
  });
  execution.artifacts = [
    // Twenty rows in, one price each; only three sets ever get a detail read.
    matrix(
      "catalog",
      [{ name: "Tower Bridge" }, { name: "Paris" }, { name: "London" }],
      ["$119.99", "$59.99", "$249.99"],
    ),
    matrix("detail", [{ name: "Tower Bridge" }], [""]),
  ] as typeof execution.artifacts;
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
  ] as typeof execution.steps;
  const withHub = {
    ...snapshot,
    elements: [
      snapshot.elements[0]!,
      {
        id: "hub",
        area: null,
        reference: {
          kind: "subject" as const,
          objective: "objective",
          execution: "execution",
          artifact: "detail",
          index: 0,
        },
      },
    ],
  };
  const items = environmentItems(withHub, [], [], new Map([["objective", state]]));
  // One hub, and its price comes from the catalogue row the read never repeated.
  expect(items.filter((item) => item.type === "subject").map((item) => item.title)).toEqual([
    "Tower Bridge",
  ]);
  expect(items.find((item) => item.id === "hub")?.facts).toEqual([
    { label: "price", value: "$119.99" },
  ]);
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

test("browser steps become page cards with frames, working links and subject links", async () => {
  const { environmentPages } = await import("../lib/project-environment");
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
  execution.artifacts = [
    {
      ...execution.artifacts[0]!,
      id: "catalog",
      data: {
        kind: "comparison_matrix",
        subjects: [{ name: "Tower Bridge", homepage: "https://shop.example/p/1" }],
        criteria: [{ name: "price", kind: { kind: "text" } }],
        cells: [
          [{ value: { kind: "text", text: "$119" }, evidence: [], general_knowledge: false }],
        ],
        notes: [],
      },
    },
  ];
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
      status: "running",
    },
  ];
  const objectiveElement = snapshot.elements.find(
    (element) => element.reference.kind === "objective",
  )!;
  const withHub = {
    ...snapshot,
    elements: [
      objectiveElement,
      {
        id: "hub",
        area: null,
        reference: {
          kind: "subject" as const,
          objective: "objective",
          execution: "execution",
          artifact: "catalog",
          index: 0,
        },
      },
    ],
  };
  const pages = environmentPages(withHub, new Map([["objective", state]]), () => [
    {
      execution: "execution",
      attempt: "attempt",
      step: "read-2",
      url: "https://shop.example/p/1",
      live: true,
      frame: { generation: 3, width: 640, height: 400 },
    },
  ]);
  expect(pages.items.map((item) => [item.id, item.page?.host, item.page?.live])).toEqual([
    [`page:${objectiveElement.id}:read-1`, "shop.example", false],
    [`page:${objectiveElement.id}:read-2`, "shop.example", true],
  ]);
  expect(pages.items[1]!.page?.frame).toMatch(/frame\/attempt\/read-2\/3$/);
  expect(pages.items[0]!.page?.frame).toBeNull();
  expect(pages.links).toEqual([
    {
      id: `page-subject:page:${objectiveElement.id}:read-1:hub`,
      source: `page:${objectiveElement.id}:read-1`,
      target: "hub",
      kind: "supports",
    },
    {
      id: `working:page:${objectiveElement.id}:read-2`,
      source: `agent:${objectiveElement.id}`,
      target: `page:${objectiveElement.id}:read-2`,
      kind: "working",
    },
  ]);
});

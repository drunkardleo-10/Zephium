import { expect, test } from "vitest";
import { projection, snapshot } from "./environment-fixtures";
import { environmentItems } from "../lib/project-environment";
import { boardOf } from "../lib/board/adapter";
test("joins exact objective and historical artifact identities without replacing originals", () => {
  // A result whose request left the canvas is a card of its own.
  const loose = { ...snapshot, elements: [snapshot.elements[1]!] };
  const items = environmentItems(loose, [], [], new Map([["objective", projection]]));
  expect(items[0]?.artifact?.content).toEqual({
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
  // The hub is its board's entity, and its price comes from the catalogue row the read never repeated.
  expect(
    environmentItems(withHub, [], [], new Map([["objective", state]])).map((item) => item.id),
  ).toEqual(["objective-card"]);
  const board = boardOf({ id: "board", executions: state.executions, elements: withHub.elements });
  expect(board.blocks).toEqual([
    expect.objectContaining({
      kind: "entity",
      id: "hub",
      entity: expect.objectContaining({ name: "Tower Bridge", price: "$119.99" }),
    }),
  ]);
});

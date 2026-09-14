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

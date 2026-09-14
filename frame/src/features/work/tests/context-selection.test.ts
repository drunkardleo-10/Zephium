import { describe, expect, test } from "vitest";
import { contextSelection, formatBytes } from "../lib/context-selection";
import type { WorkEnvironmentSnapshot, WorkRuntimeProjection } from "$shared/ipc/bindings";
import { tabFixture } from "$shared/testing/fixtures";

const snapshot: WorkEnvironmentSnapshot = {
  version: 1,
  id: "env",
  profile: "p",
  space: "s",
  title: "T",
  lifecycle: "active",
  revision: "3",
  elements: [
    { id: "e-note", area: null, reference: { kind: "resource", resource: "note-1" } },
    { id: "e-tab", area: null, reference: { kind: "browser", tab: "tab-1" } },
    { id: "e-obj", area: null, reference: { kind: "objective", objective: "work-1" } },
    {
      id: "e-art",
      area: null,
      reference: { kind: "artifact", objective: "work-1", execution: "x", artifact: "art-9" },
    },
    { id: "e-unknown", area: null, reference: { kind: "resource", resource: "note-missing" } },
  ],
  areas: [],
  view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
};

describe("contextSelection", () => {
  test("carries the identity token Rust recomputes for each selected element", () => {
    const objectives = new Map<string, WorkRuntimeProjection>([
      ["work-1", { work: { revision: "7" } } as unknown as WorkRuntimeProjection],
    ]);
    const selection = contextSelection(
      snapshot,
      ["e-note", "e-tab", "e-obj", "e-art", "e-unknown", "not-an-element"],
      [tabFixture({ id: "tab-1", url: "https://docs.example/a" })],
      {
        notes: [
          {
            id: "note-1",
            revision: "12",
            title: "N",
            pinned: false,
            updated_at: "",
            completed: null,
            due_date: null,
          },
        ],
        objectives,
      },
    );
    expect(selection).toEqual({
      environment: "env",
      items: [
        { element: "e-note", revision: "12" },
        { element: "e-tab", revision: "https://docs.example/a" },
        { element: "e-obj", revision: "7" },
        { element: "e-art", revision: "art-9" },
      ],
    });
  });

  test("is null without a snapshot, a selection, or any resolvable token", () => {
    expect(contextSelection(null, ["e-note"], [], { notes: [], objectives: new Map() })).toBeNull();
    expect(contextSelection(snapshot, [], [], { notes: [], objectives: new Map() })).toBeNull();
    expect(
      contextSelection(snapshot, ["e-unknown"], [], { notes: [], objectives: new Map() }),
    ).toBeNull();
  });

  test("formats sizes for the manifest", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(20 * 1024)).toBe("20 KB");
  });
});

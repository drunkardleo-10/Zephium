import { describe, expect, it } from "vitest";
import type { ToolKind } from "$shared/ipc/bindings";
import { toolSession, editTool } from "../tool-drafts.svelte";
describe("shared tool views with independent session state", () => {
  it("preserves a draft after a view unmounts without sharing it across profiles", () => {
    const notes = toolSession("profile-a", "notes");
    editTool(notes, { draft: "A local draft", scrollTop: 120 });
    expect(toolSession("profile-a", "notes").draft).toBe("A local draft");
    expect(toolSession("profile-b", "notes").draft).toBe("");
  });
  it("bounds retained text and normalizes invalid scroll values", () => {
    const state = toolSession("bounded", "ai");
    editTool(state, { draft: "x".repeat(20000), query: "x".repeat(2000), scrollTop: NaN });
    expect(state.draft).toHaveLength(16384);
    expect(state.query).toHaveLength(1024);
    expect(state.scrollTop).toBe(0);
  });
  it("evicts the least recently used draft, never the one in use", () => {
    const open = toolSession("in-use", "notes");
    editTool(open, { draft: "Still being written" });
    const tools: ToolKind[] = ["notes", "tasks", "ai", "history", "downloads", "time"];
    for (let i = 0; i < 30; i++) {
      toolSession(`other-${i}`, tools[i % tools.length]!);
      // The mounted view reads its draft as it renders.
      toolSession("in-use", "notes");
    }
    expect(toolSession("in-use", "notes").draft).toBe("Still being written");
    expect(toolSession("other-0", "notes").draft).toBe("");
  });
});

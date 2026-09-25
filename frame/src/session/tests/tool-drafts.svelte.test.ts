import { describe, expect, it } from "vitest";
import { toolSession, editTool } from "../tool-drafts.svelte";
describe("shared tool views with independent session state", () => {
  it("preserves a draft after a view unmounts without sharing it across hosts", () => {
    const floating = toolSession("floating", "profile-a", "notes");
    editTool(floating, { draft: "A local draft", scrollTop: 120 });
    expect(toolSession("floating", "profile-a", "notes").draft).toBe("A local draft");
    expect(toolSession("sidebar", "profile-a", "notes").draft).toBe("");
    expect(toolSession("floating", "profile-b", "notes").draft).toBe("");
  });
  it("bounds retained text and normalizes invalid scroll values", () => {
    const state = toolSession("floating", "bounded", "ai");
    editTool(state, { draft: "x".repeat(20000), query: "x".repeat(2000), scrollTop: NaN });
    expect(state.draft).toHaveLength(16384);
    expect(state.query).toHaveLength(1024);
    expect(state.scrollTop).toBe(0);
  });
});

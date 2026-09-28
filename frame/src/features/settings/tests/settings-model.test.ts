import { describe, expect, it } from "vitest";
import { searchSections } from "../lib/settings-model";

describe("settings discovery", () => {
  it("finds nested preferences by their localized vocabulary", () => {
    expect(searchSections("languages").map((section) => section.id)).toContain("languages");
    expect(searchSections("motion").map((section) => section.id)).toContain("appearance");
    expect(searchSections("passwords").map((section) => section.id)).toContain("passwords");
  });
  it("matches every search word and distinguishes no results", () => {
    expect(searchSections("  ACCENT color  ").map((section) => section.id)).toEqual(["appearance"]);
    expect(searchSections("accent downloads")).toEqual([]);
    expect(searchSections("unfindablephrase")).toEqual([]);
  });
});

it("finds the reserved agent destinations without exposing unimplemented controls", async () => {
  const { searchSettings, searchSections, emptySections } = await import("../lib/settings-model");
  for (const id of ["plugins"] as const) expect(emptySections.has(id)).toBe(true);
  expect(emptySections.has("ai")).toBe(false);
  expect(searchSections("models").some((section) => section.id === "ai")).toBe(true);
  expect(searchSections("MCP").some((section) => section.id === "mcp")).toBe(true);
  expect(searchSettings("API key")).toEqual([]);
  expect(searchSettings("updates").some((result) => result.target === "updates.check")).toBe(true);
});

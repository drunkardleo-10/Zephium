import { expect, test } from "vitest";
import { publicResearchQueryValid } from "../lib/public-research";
test("public research validates Unicode code points and UTF8 without truncation", () => {
  expect(publicResearchQueryValid("🔬".repeat(512))).toBe(true);
  expect(publicResearchQueryValid("🔬".repeat(513))).toBe(false);
  expect(publicResearchQueryValid("a".repeat(513))).toBe(false);
  expect(publicResearchQueryValid("")).toBe(false);
  expect(publicResearchQueryValid("Find cafés in Łódź")).toBe(true);
});

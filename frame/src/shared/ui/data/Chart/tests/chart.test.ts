import { expect, test } from "vitest";
import { axisTicks, basisText, plotPoints } from "../chart";
test("preserves original decimals while generating approximate plot coordinates", () => {
  const points = [{ label: "one", value: "42.5000" }];
  expect(plotPoints(points)).toEqual([{ label: "one", value: 42.5 }]);
  expect(points[0]!.value).toBe("42.5000");
});
test.each(["NaN", "Infinity", "0x12", "9007199254740993", "1e999"])(
  "does not plot unsupported value %s",
  (value) => {
    expect(plotPoints([{ label: "one", value }])).toBeNull();
  },
);
test("does not merge duplicate categorical labels into a misleading plot", () => {
  expect(
    plotPoints([
      { label: "one", value: "1" },
      { label: "one", value: "2" },
    ]),
  ).toBeNull();
});
test("axis ticks step in human amounts and always include zero", () => {
  expect(axisTicks(0, 37)).toEqual([0, 10, 20, 30, 40]);
  expect(axisTicks(120, 480)).toContain(0);
  expect(axisTicks(0, 0)).toEqual([0]);
});
test("a stated basis reads as one line, an unstated one as nothing", () => {
  expect(basisText({ method: "Same rig", conditions: "", observedAt: "2026-09-16" })).toBe(
    "Same rig · 2026-09-16",
  );
  expect(basisText(undefined)).toBe("");
});

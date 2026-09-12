import { expect, test } from "vitest";
import { plotPoints } from "../chart";
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

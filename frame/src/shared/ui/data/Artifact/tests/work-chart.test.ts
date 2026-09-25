import { expect, test } from "vitest";
import { basisText, workChartSpec } from "../work-chart";

const chart = (values: string[][], extra = {}) => ({
  xLabel: "Model",
  yLabel: "Price",
  series: values.map((points, order) => ({
    name: `S${order}`,
    points: points.map((value, index) => ({ label: `L${index}`, value })),
  })),
  ...extra,
});

test("exact decimals become numbers once and keep their own words", () => {
  const spec = workChartSpec(chart([["2.5000", "3.75"]]));
  expect(spec.kind).toBe("bars");
  expect(spec.x).toEqual({ label: "Model", kind: "category" });
  expect(spec.y).toEqual({ label: "Price", format: "number" });
  expect(spec.series[0]!.points).toEqual([
    { x: "L0", y: 2.5, display: "2.5000" },
    { x: "L1", y: 3.75, display: "3.75" },
  ]);
});

test("a range string makes a range chart", () => {
  const spec = workChartSpec(chart([["$3,000–8,000", "$1,200"]]));
  expect(spec.kind).toBe("range");
  expect(spec.y).toMatchObject({ format: "money", currency: "USD" });
  expect(spec.series[0]!.points[0]).toMatchObject({ y: 3000, y2: 8000, display: "$3,000–8,000" });
  expect(workChartSpec(chart([["3000-8000"]])).kind).toBe("range");
});

test("an unknown value is a gap, never a guess", () => {
  const spec = workChartSpec(chart([["unknown", "9007199254740993", "4"]]));
  expect(spec.series[0]!.points.map((point) => point.y)).toEqual([null, null, 4]);
  expect(spec.series[0]!.points[1]!.display).toBe("9007199254740993");
});

test("duplicate labels are not merged into one misleading bar", () => {
  const spec = workChartSpec({
    xLabel: "X",
    yLabel: "Y",
    series: [
      {
        name: "S",
        points: [
          { label: "a", value: "1" },
          { label: "a", value: "2" },
        ],
      },
    ],
  });
  expect(spec.series[0]!.points.every((point) => point.y === null)).toBe(true);
});

test("evidence, basis, knowledge and the card", () => {
  const spec = workChartSpec({
    xLabel: "X",
    yLabel: "Y",
    series: [
      {
        name: "S",
        points: [
          {
            label: "a",
            value: "1",
            evidence: [
              { key: "k", label: "Bench", origin: "b.io", file: { record: "r", path: "p" } },
            ],
          },
        ],
      },
    ],
    basis: "Basis: Same rig",
    generalKnowledge: true,
  });
  expect(spec.series[0]!.points[0]!.evidence).toEqual([
    { key: "k", label: "Bench", origin: "b.io", file: true },
  ]);
  expect(spec.basis).toBe("Basis: Same rig");
  expect(spec.knowledge).toBe(true);
  const card = workChartSpec({ xLabel: "X", yLabel: "Y", series: [], basis: "b", compact: true });
  expect(card.basis).toBeUndefined();
  expect(card.compact).toBe(true);
});

test("a long series reads as a trend; percentages keep their unit", () => {
  const long = Array.from({ length: 30 }, (_, i) => `${i}%`);
  const spec = workChartSpec(chart([long]));
  expect(spec.kind).toBe("line");
  expect(spec.y?.format).toBe("percent");
});

test("a stated basis reads as one line, an unstated one as nothing", () => {
  expect(basisText({ method: "Same rig", conditions: "", observedAt: "2026-09-16" })).toBe(
    "Same rig · 2026-09-16",
  );
  expect(basisText(undefined)).toBe("");
});

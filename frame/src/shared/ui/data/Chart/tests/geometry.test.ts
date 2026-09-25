import { expect, test } from "vitest";
import type { ChartSpec } from "../chart";
import { barPath, categoryKeys, donutParts, geometry, horizontal, readout } from "../geometry";

const bars = (points: [string, number | null][], extra: Partial<ChartSpec> = {}): ChartSpec => ({
  kind: "bars",
  series: [{ name: "Price", points: points.map(([x, y]) => ({ x, y })) }],
  ...extra,
});

test("bars stand on a zero baseline with rounded ticks and one hover target per category", () => {
  const shape = geometry(
    bars([
      ["A", 12],
      ["B", 37],
      ["C", null],
    ]),
    400,
  );
  expect(shape.ticks.map((tick) => tick.label)).toEqual(["0", "10", "20", "30", "40"]);
  expect(shape.bars).toHaveLength(2);
  expect(shape.hits).toHaveLength(3);
  const [a, b] = shape.bars;
  // Taller value, higher tip; both grow from the same baseline.
  expect(b!.tip.y).toBeLessThan(a!.tip.y);
  expect(a!.origin.y).toBe(b!.origin.y);
  expect(shape.ticks.find((tick) => tick.zero)!.pos).toBe(a!.origin.y);
});

test("bars lie down when there are many or their names are long, never on a card", () => {
  const many = bars(Array.from({ length: 9 }, (_, i) => [`C${i}`, i] as [string, number]));
  expect(horizontal(many, categoryKeys(many))).toBe(true);
  const long = bars([["A very long category name", 1]]);
  expect(horizontal(long, categoryKeys(long))).toBe(true);
  expect(horizontal({ ...many, compact: true }, categoryKeys(many))).toBe(false);
  const shape = geometry(many, 400);
  expect(shape.valueAxis).toBe("x");
  expect(shape.rows).toHaveLength(9);
});

test("a range spans its two ends", () => {
  const shape = geometry(
    { kind: "range", series: [{ name: "Quote", points: [{ x: "A", y: 3000, y2: 8000 }] }] },
    400,
  );
  expect(shape.ticks.at(-1)!.label).toBe("8K");
  const bar = shape.bars[0]!;
  expect(bar.origin.y).toBeGreaterThan(bar.tip.y);
  expect(bar.origin.y).toBeLessThan(shape.ticks[0]!.pos);
});

test("a stack sums each category and parts segments with a surface gap", () => {
  const shape = geometry(
    {
      kind: "stacked",
      series: [
        {
          name: "Read",
          points: [
            { x: "Mon", y: 30 },
            { x: "Tue", y: 10 },
          ],
        },
        {
          name: "Write",
          points: [
            { x: "Mon", y: 20 },
            { x: "Tue", y: 5 },
          ],
        },
      ],
    },
    400,
  );
  expect(shape.ticks.map((tick) => tick.label)).toEqual(["0", "20", "40", "60"]);
  expect(shape.bars).toHaveLength(4);
  const top = shape.bars.find((bar) => bar.series === 1 && bar.index === 0)!;
  const bottom = shape.bars.find((bar) => bar.series === 0 && bar.index === 0)!;
  expect(top.tip.y).toBeLessThan(bottom.tip.y);
});

test("a line keeps its own range, gaps at null, and hovers per x", () => {
  const shape = geometry(
    {
      kind: "line",
      series: [
        {
          name: "Nightly",
          points: [
            { x: "Jan", y: 120 },
            { x: "Feb", y: null },
            { x: "Mar", y: 150 },
          ],
        },
      ],
    },
    400,
  );
  expect(shape.ticks[0]!.label).not.toBe("0");
  expect(shape.traces[0]!.dots).toHaveLength(2);
  expect(shape.traces[0]!.line.match(/M/gu)).toHaveLength(2);
  expect(shape.hits).toHaveLength(3);
  expect(shape.cursor).toBe(true);
});

test("an area fills to zero and a time axis reads in date order", () => {
  const spec: ChartSpec = {
    kind: "area",
    x: { kind: "time" },
    series: [
      {
        name: "Time",
        points: [
          { x: "2026-09-03", y: 5 },
          { x: "2026-09-01", y: 3 },
          { x: "2026-09-02", y: 4 },
        ],
      },
    ],
  };
  expect(categoryKeys(spec)).toEqual(["2026-09-01", "2026-09-02", "2026-09-03"]);
  const shape = geometry(spec, 400);
  expect(shape.ticks[0]!.label).toBe("0");
  expect(shape.traces[0]!.area).toBeTruthy();
  expect(shape.labels.length).toBeGreaterThan(0);
});

test("a donut keeps eight slices and folds the rest into Other", () => {
  const spec: ChartSpec = {
    kind: "donut",
    series: [
      { name: "Sites", points: Array.from({ length: 10 }, (_, i) => ({ x: `s${i}`, y: 10 - i })) },
    ],
  };
  const parts = donutParts(spec, "Other");
  expect(parts).toHaveLength(8);
  expect(parts.at(-1)).toMatchObject({ label: "Other", value: 3 + 2 + 1, other: true });
  const shape = geometry(spec, 300, { other: "Other" });
  expect(shape.slices).toHaveLength(8);
  expect(shape.total).toBe(55);
  expect(shape.slices.at(-1)!.color).toBe("var(--color-tint-graphite)");
  expect(readout(spec, shape, 0, "Other")!.rows[0]!.name).toBe("18.2%");
});

test("a heat grid is series by category on a single tone ramp", () => {
  const shape = geometry(
    {
      kind: "heat",
      series: [
        {
          name: "Mon",
          points: [
            { x: "9", y: 0 },
            { x: "10", y: 4 },
          ],
        },
        {
          name: "Tue",
          points: [
            { x: "9", y: 2 },
            { x: "10", y: null },
          ],
        },
      ],
    },
    300,
  );
  expect(shape.cells.map((cell) => cell.tone)).toEqual([0, 1, 0.5, null]);
  expect(shape.rows.map((row) => row.text)).toEqual(["Mon", "Tue"]);
});

test("a spark has no axes", () => {
  const shape = geometry(
    {
      kind: "spark",
      series: [
        {
          name: "",
          points: [
            { x: 1, y: 1 },
            { x: 2, y: 3 },
          ],
        },
      ],
    },
    80,
    { sparkHeight: 20 },
  );
  expect(shape.height).toBe(20);
  expect(shape.ticks).toHaveLength(0);
  expect(shape.labels).toHaveLength(0);
  expect(shape.traces[0]!.line).toMatch(/^M/u);
});

test("compact drops tick and category labels", () => {
  const shape = geometry(
    bars(
      [
        ["A", 1],
        ["B", 2],
      ],
      { compact: true },
    ),
    300,
  );
  expect(shape.ticks.every((tick) => tick.label === "")).toBe(true);
  expect(shape.labels).toHaveLength(0);
  expect(shape.height).toBe(132);
});

test("a bar rounds only its data end", () => {
  expect(barPath(0, 0, 24, 40, "top")).toMatch(/^M0,40v-36a4,4/u);
  expect(barPath(0, 0, 24, 40, "none")).toBe("M0,0h24v40h-24Z");
});

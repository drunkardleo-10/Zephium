import { expect, test } from "vitest";
import type { ChartSpec } from "../chart";
import {
  card,
  categoryKeys,
  donutParts,
  horizontal,
  legendRule,
  plan,
  radar,
  ringTop,
  rings,
  readout,
  shortValue,
  valuesCsv,
  type PlanSeries,
  type Row,
} from "../layer";

const bars = (points: [string, number | null][], extra: Partial<ChartSpec> = {}): ChartSpec => ({
  kind: "bars",
  series: [{ name: "Price", points: points.map(([x, y]) => ({ x, y })) }],
  ...extra,
});

const read = (series: PlanSeries, row: Row) =>
  Array.isArray(series.value) ? series.value.map((get) => get(row)) : series.value(row);

test("bars: one row per category, a grouped series per spec series, round ticks from zero", () => {
  const shape = plan(
    bars([
      ["A", 12],
      ["B", 37],
      ["C", null],
    ]),
  );
  expect(shape.rows.map((row) => row.x)).toEqual(["A", "B", "C"]);
  expect(shape.scale).toBe("band");
  expect(shape.layout).toBe("group");
  expect(shape.ticks).toEqual([0, 10, 20, 30, 40]);
  expect(shape.domain).toEqual([0, 40]);
  const [series] = shape.series;
  expect(series).toMatchObject({ key: "s0", label: "Price", color: "var(--chart-1)" });
  // A null stays a gap: nothing is drawn and nothing invented.
  expect(shape.rows.map((row) => read(series!, row))).toEqual([12, 37, null]);
});

test("stacked: diverging layout, a missing part stacks as nothing, ticks span the sums", () => {
  const shape = plan({
    kind: "stacked",
    series: [
      {
        name: "Read",
        points: [
          { x: "Mon", y: 30 },
          { x: "Tue", y: 10 },
        ],
      },
      { name: "Write", points: [{ x: "Mon", y: 20 }] },
    ],
  });
  expect(shape.layout).toBe("stackDiverging");
  expect(shape.ticks).toEqual([0, 20, 40, 60]);
  expect(shape.rows.map((row) => read(shape.series[1]!, row))).toEqual([20, 0]);
});

test("range: each series reads low and high", () => {
  const shape = plan({
    kind: "range",
    series: [{ name: "Quote", points: [{ x: "A", y: 3000, y2: 8000 }] }],
  });
  expect(read(shape.series[0]!, shape.rows[0]!)).toEqual([3000, 8000]);
  expect(shape.domain).toEqual([0, 8000]);
});

test("line: its own range; a time axis reads Dates in order", () => {
  const line = plan({
    kind: "line",
    series: [
      {
        name: "Nightly",
        points: [
          { x: "Jan", y: 120 },
          { x: "Feb", y: 150 },
        ],
      },
    ],
  });
  expect(line.domain[0]).toBeGreaterThan(0);
  expect(line.layout).toBe("overlap");
  const spec: ChartSpec = {
    kind: "area",
    x: { kind: "time" },
    series: [
      {
        name: "Time",
        points: [
          { x: "2026-09-03", y: 5 },
          { x: "2026-09-01", y: 3 },
        ],
      },
    ],
  };
  expect(categoryKeys(spec)).toEqual(["2026-09-01", "2026-09-03"]);
  const area = plan(spec);
  expect(area.scale).toBe("time");
  expect(area.rows[0]!.x).toBeInstanceOf(Date);
  expect((area.rows[0]!.x as Date).getDate()).toBe(1);
  expect(area.domain[0]).toBe(0);
});

test("donut: largest first, eight slices, the rest as Other in the neutral tone", () => {
  const spec: ChartSpec = {
    kind: "donut",
    series: [
      { name: "Sites", points: Array.from({ length: 10 }, (_, i) => ({ x: `s${i}`, y: 10 - i })) },
    ],
  };
  const parts = donutParts(spec, "Other");
  expect(parts).toHaveLength(8);
  expect(parts.at(-1)).toMatchObject({
    label: "Other",
    value: 6,
    color: "var(--color-tint-graphite)",
  });
  const shape = plan(spec, "Other");
  expect(shape.total).toBe(55);
  expect(shape.parts[0]!.color).toBe("var(--chart-1)");
  expect(readout(spec, shape, 0)!.rows[0]!.name).toBe("18.2%");
});

test("heat: a cell per series and category on one tone ramp", () => {
  const shape = plan({
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
  });
  expect(shape.cells.map((cell) => [cell.key, cell.order, cell.tone])).toEqual([
    ["9", "0", 0],
    ["10", "0", 1],
    ["9", "1", 0.5],
    ["10", "1", null],
  ]);
});

test("bars lie down when there are many or their names are long, never on a card", () => {
  const many = bars(Array.from({ length: 9 }, (_, i) => [`C${i}`, i] as [string, number]));
  expect(horizontal(many, categoryKeys(many))).toBe(true);
  const long = bars([["A very long category name", 1]]);
  expect(horizontal(long, categoryKeys(long))).toBe(true);
  expect(horizontal({ ...many, compact: true }, categoryKeys(many))).toBe(false);
  expect(plan(many).horizontal).toBe(true);
});

test("the tooltip lists every series at the hovered x with its sources", () => {
  const spec: ChartSpec = {
    kind: "bars",
    series: [
      { name: "A", points: [{ x: "Mon", y: 1, evidence: [{ key: "k", label: "Bench" }] }] },
      { name: "B", points: [{ x: "Mon", y: 2 }] },
    ],
  };
  expect(readout(spec, plan(spec), 0)).toEqual({
    title: "Mon",
    rows: [
      { name: "A", color: "var(--chart-1)", text: "1" },
      { name: "B", color: "var(--chart-2)", text: "2" },
    ],
    evidence: [{ key: "k", label: "Bench" }],
  });
});

test("a card abbreviates with the spec's format", () => {
  expect(shortValue({ x: "a", y: 1234 })).toBe("1.2K");
  expect(shortValue({ x: "a", y: 3400 }, { format: "money", currency: "USD" })).toBe("$3.4K");
  expect(shortValue({ x: "a", y: 45 }, { format: "percent" })).toBe("45%");
  expect(shortValue({ x: "a", y: 3000, y2: 8000 }, { format: "money", currency: "USD" })).toBe(
    "$3K–$8K",
  );
  expect(shortValue({ x: "a", y: null })).toBe("");
});

test("card rules: bars carry labels and values, trends their ends, a donut its lead", () => {
  const priced = bars(
    [
      ["A", 12],
      ["B", 37],
    ],
    { compact: true },
  );
  expect(card(priced, plan(priced))).toEqual({
    categories: true,
    values: true,
    ends: null,
    lead: null,
    legend: "none",
  });
  const crowded = bars(
    Array.from({ length: 13 }, (_, i) => [`C${i}`, i] as [string, number]),
    { compact: true },
  );
  expect(card(crowded, plan(crowded)).values).toBe(false);
  const line: ChartSpec = {
    kind: "line",
    compact: true,
    series: [
      {
        name: "S",
        points: [
          { x: "Jan", y: 1 },
          { x: "Feb", y: 1500 },
          { x: "Mar", y: null },
        ],
      },
    ],
  };
  expect(card(line, plan(line)).ends).toEqual({ first: "Jan", last: "Mar", value: "1.5K" });
  const donut: ChartSpec = {
    kind: "donut",
    compact: true,
    series: [
      {
        name: "S",
        points: [
          { x: "Mail", y: 1 },
          { x: "Docs", y: 3 },
        ],
      },
    ],
  };
  expect(card(donut, plan(donut)).lead).toEqual({ label: "Docs", share: "75%" });
});

test("legend rule: none for one series, two rows for two to four, the table past four", () => {
  expect([1, 2, 4, 5].map((count) => legendRule(count, true))).toEqual([
    "none",
    "rows",
    "rows",
    "table",
  ]);
  expect(legendRule(6, false)).toBe("rows");
});

test("values copy as CSV in the source's own words", () => {
  expect(
    valuesCsv({
      kind: "bars",
      x: { label: "Builder" },
      series: [
        {
          name: "Quote, total",
          points: [
            { x: "Acme", y: 3000, display: "$3,000" },
            { x: "Bolt", y: null },
          ],
        },
      ],
    }),
  ).toBe('Builder,"Quote, total"\nAcme,"$3,000"\nBolt,');
});

test("a trend over categories stands them at even steps, edge to edge", () => {
  const shape = plan({
    kind: "area",
    stack: true,
    series: [
      {
        name: "A",
        points: [
          { x: "Jan", y: 1 },
          { x: "Feb", y: 2 },
        ],
      },
      {
        name: "B",
        points: [
          { x: "Jan", y: 3 },
          { x: "Feb", y: 4 },
        ],
      },
    ],
  });
  expect(shape.scale).toBe("index");
  expect(shape.rows.map((row) => row.x)).toEqual([0, 1]);
  expect(shape.layout).toBe("stack");
  expect(shape.domain[1]).toBeGreaterThanOrEqual(6);
});

test("radial: a ring per value against the scale's top, percent out of 100", () => {
  const spec = {
    kind: "radial" as const,
    y: { format: "percent" as const },
    series: [
      {
        name: "Done",
        points: [
          { x: "Docs", y: 60 },
          { x: "Billing", y: 75 },
        ],
      },
    ],
  };
  const parts = rings(spec);
  expect(parts.map((part) => [part.label, part.value])).toEqual([
    ["Docs", 60],
    ["Billing", 75],
  ]);
  expect(ringTop(spec, parts)).toBe(100);
  expect(ringTop({ ...spec, y: {} }, parts)).toBe(75);
});

test("radar: a spoke per category, reach against a stated scale", () => {
  const shape = radar({
    kind: "radar",
    y: { max: 5 },
    series: [
      {
        name: "AWS",
        points: [
          { x: "Price", y: 2 },
          { x: "Edge", y: 5 },
          { x: "Ops", y: null },
        ],
      },
    ],
  });
  expect(shape.axes.map((axis) => axis.label)).toEqual(["Price", "Edge", "Ops"]);
  expect(shape.ticks).toEqual([1, 2, 3, 4, 5]);
  expect(shape.series[0]!.reach).toEqual([0.4, 1, null]);
});

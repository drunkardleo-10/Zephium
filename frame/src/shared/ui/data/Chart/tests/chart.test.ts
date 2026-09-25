import { expect, test } from "vitest";
import {
  axisTicks,
  durationTicks,
  extremes,
  formatBytes,
  formatDuration,
  formatValue,
  parseTime,
  parseValue,
  pointText,
  tickLabel,
} from "../chart";

test("parses exact decimals and refuses what a plot would misstate", () => {
  expect(parseValue("42.5000")).toEqual({ y: 42.5 });
  expect(parseValue(" -3 ")).toEqual({ y: -3 });
  expect(parseValue("1,200.5")).toEqual({ y: 1200.5 });
  expect(parseValue("$1,299")).toEqual({ y: 1299, currency: "USD" });
  expect(parseValue("12%")).toEqual({ y: 12, percent: true });
});

test.each(["NaN", "Infinity", "0x12", "9007199254740993", "1e999", "1e5", "", "about 3", "1,20"])(
  "does not plot unsupported value %j",
  (value) => {
    expect(parseValue(value)).toBeNull();
  },
);

test("reads a range however it was written, low end first", () => {
  expect(parseValue("3000-8000")).toEqual({ y: 3000, y2: 8000 });
  expect(parseValue("$3,000–8,000")).toEqual({ y: 3000, y2: 8000, currency: "USD" });
  expect(parseValue("$3,000 – $8,000")).toEqual({ y: 3000, y2: 8000, currency: "USD" });
  expect(parseValue("8 to 3")).toEqual({ y: 3, y2: 8 });
  expect(parseValue("-5-10")).toEqual({ y: -5, y2: 10 });
  expect(parseValue("$3–€8")).toBeNull();
});

test("formats each unit the way a reader says it", () => {
  expect(formatValue(1234.5)).toBe("1,234.5");
  expect(formatValue(12, { unit: "ms" })).toBe("12 ms");
  expect(formatValue(1299, { format: "money", currency: "USD" })).toBe("$1,299");
  expect(formatValue(12.5, { format: "money", currency: "EUR" })).toBe("€12.50");
  expect(formatValue(42, { format: "percent" })).toBe("42%");
  expect(formatValue(5040, { format: "duration" })).toBe("1h 24m");
  expect(formatValue(1_200_000_000, { format: "bytes" })).toBe("1.2 GB");
  // A money format without a currency falls back to a plain number.
  expect(formatValue(3, { format: "money" })).toBe("3");
});

test("durations and bytes", () => {
  expect(formatDuration(45)).toBe("45s");
  expect(formatDuration(200)).toBe("3m 20s");
  expect(formatDuration(720)).toBe("12m");
  expect(formatDuration(3600)).toBe("1h");
  expect(formatBytes(999)).toBe("999 B");
  expect(formatBytes(340_000_000)).toBe("340 MB");
});

test("axis ticks step in human amounts and include zero unless asked not to", () => {
  expect(axisTicks(0, 37)).toEqual([0, 10, 20, 30, 40]);
  expect(axisTicks(120, 480)).toContain(0);
  expect(axisTicks(0, 0)).toEqual([0]);
  expect(axisTicks(120, 150, 4, false)).toEqual([120, 130, 140, 150]);
  expect(durationTicks(0, 5000)).toEqual([0, 1800, 3600, 5400]);
  expect(tickLabel(40_000)).toBe("40K");
  expect(tickLabel(3600, { format: "duration" })).toBe("1h");
  expect(tickLabel(1500, { format: "money", currency: "USD" })).toBe("$1.5K");
});

test("a point reads in its own words first, then as a value or a range", () => {
  expect(pointText({ x: "a", y: 2.5, display: "2.5000" })).toBe("2.5000");
  expect(pointText({ x: "a", y: 3000, y2: 8000 }, { format: "money", currency: "USD" })).toBe(
    "$3,000–$8,000",
  );
  expect(pointText({ x: "a", y: null })).toBe("—");
});

test("the extremes name where the lowest and highest values are", () => {
  expect(
    extremes({
      kind: "range",
      series: [
        {
          name: "Quotes",
          points: [
            { x: "A", y: 3000, y2: 8000 },
            { x: "B", y: 1000, y2: 2000 },
            { x: "C", y: null },
          ],
        },
      ],
    }),
  ).toEqual({ low: { x: "B", value: "1,000" }, high: { x: "A", value: "8,000" } });
  expect(extremes({ kind: "bars", series: [{ name: "", points: [{ x: "A", y: null }] }] })).toBe(
    null,
  );
});

test("a bare date is the local day, not UTC's", () => {
  expect(new Date(parseTime("2026-09-01")).getDate()).toBe(1);
  expect(parseTime("2026-09-01T10:00:00Z")).toBe(Date.UTC(2026, 8, 1, 10));
});

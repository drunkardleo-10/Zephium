import { describe, expect, it } from "vitest";
import { offsetsOf, rowAt, totalHeight, windowFor } from "../virtual-window.svelte";

const HEIGHTS = { day: 40, visit: 30 };

function heights(pattern: ("day" | "visit")[], repeat: number): number[] {
  return Array.from({ length: repeat }, () => pattern)
    .flat()
    .map((kind) => HEIGHTS[kind]);
}

describe("windowing offsets", () => {
  it("opens each row where the previous one ends", () => {
    const offsets = offsetsOf(heights(["day", "visit", "visit", "day"], 1));
    expect(offsets).toEqual([0, 40, 70, 100, 140]);
    expect(totalHeight(offsets)).toBe(140);
  });

  it("handles an empty list without inventing a row", () => {
    const offsets = offsetsOf([]);
    expect(totalHeight(offsets)).toBe(0);
    expect(windowFor(offsets, 0, 500)).toEqual({ first: 0, last: 0, before: 0, after: 0 });
  });

  it("finds the row covering an offset, including its exact top edge", () => {
    const offsets = offsetsOf(heights(["day", "visit", "visit", "visit"], 1));
    expect(rowAt(offsets, 0)).toBe(0);
    expect(rowAt(offsets, 39)).toBe(0);
    expect(rowAt(offsets, 40)).toBe(1);
    expect(rowAt(offsets, 71)).toBe(2);
    // Past the end clamps to the last row rather than reading off the array.
    expect(rowAt(offsets, 10_000)).toBe(3);
  });
});

describe("the rendered window", () => {
  const rows = heights(["day", "visit", "visit", "visit", "visit"], 200);
  const offsets = offsetsOf(rows);

  it("covers the viewport and leaves the rest as spacer height", () => {
    const slice = windowFor(offsets, 5000, 600);

    expect(slice.first).toBeLessThanOrEqual(rowAt(offsets, 5000));
    expect(slice.last).toBeGreaterThan(rowAt(offsets, 5600));
    expect(slice.before + slice.after).toBe(
      totalHeight(offsets) - (offsets[slice.last]! - offsets[slice.first]!),
    );
  });

  it("renders a bounded slice however long the list is", () => {
    const slice = windowFor(offsets, 5000, 600);
    expect(slice.last - slice.first).toBeLessThan(60);
    expect(rows.length).toBe(1000);
  });

  it("clamps at both ends rather than indexing outside the list", () => {
    expect(windowFor(offsets, -500, 600).first).toBe(0);
    const end = windowFor(offsets, totalHeight(offsets), 600);
    expect(end.last).toBe(rows.length);
    expect(end.after).toBe(0);
  });

  it("draws beyond each edge so a fast scroll finds rows already there", () => {
    const tight = windowFor(offsets, 5000, 600, 0);
    const padded = windowFor(offsets, 5000, 600, 8);
    expect(padded.first).toBeLessThan(tight.first);
    expect(padded.last).toBeGreaterThan(tight.last);
  });
});

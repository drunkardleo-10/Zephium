import { describe, expect, it } from "vitest";
import { layout } from "../lib/layout";

const FULL = { controls: 2, clock: true, tiles: 3 };

describe("layout", () => {
  it("sets the figures along the foot and the name in the middle of the rest", () => {
    const page = layout(1200, 800, FULL);
    expect(page.tiles).toHaveLength(3);
    expect(page.tiles.every((tile) => tile.y + tile.h === 800 - 28)).toBe(true);
    const [first, , last] = page.tiles;
    // As far in from the left edge as from the right.
    expect(first!.x + last!.x + last!.w).toBe(1200);
    expect(page.mark.x + page.mark.w / 2).toBeCloseTo(600, 0);
    expect(page.when).toBe(page.mark.y + page.mark.h + 26);
    expect(page.when! + 18).toBeLessThan(first!.y);
  });

  it("gives each control its own round button in the top corner", () => {
    const page = layout(700, 800, FULL);
    expect(page.notch.width).toBe(480);
    expect(page.controls).toEqual([
      { x: 700 - 14 - 32 - 38, y: 9, w: 32, h: 32 },
      { x: 700 - 14 - 32, y: 9, w: 32, h: 32 },
    ]);
  });

  it("narrows the figures with the page rather than breaking the row", () => {
    const page = layout(560, 800, FULL);
    const [, , last] = page.tiles;
    expect(last!.x + last!.w).toBeLessThanOrEqual(560 - 40);
    expect(new Set(page.tiles.map((tile) => tile.y)).size).toBe(1);
  });

  it("closes up around what is not shown", () => {
    const page = layout(1200, 800, { controls: 0, clock: false, tiles: 0 });
    expect(page.controls).toEqual([]);
    expect(page.when).toBeNull();
    expect(page.tiles).toEqual([]);
  });

  it("never lets the name ride up under the notch on a short page", () => {
    expect(layout(1200, 300, FULL).mark.y).toBe(90);
  });
});

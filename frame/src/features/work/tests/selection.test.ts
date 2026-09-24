import { expect, test } from "vitest";
import { grid, pack } from "../lib/arrange";
import type { CanvasItem, WorkNode } from "../lib/canvas-model";
import { alignTo, arrange, fitArea, moveTo, selectedPlacements } from "../lib/selection";

const item = (id: string): CanvasItem => ({ id, title: id, kind: "", detail: "", status: "" });
const card = (id: string, x: number, y: number, parentId?: string): WorkNode => ({
  id,
  type: "work",
  position: { x, y },
  width: 100,
  height: 60,
  data: item(id),
  ...(parentId ? { parentId } : {}),
});
const area = (x: number, y: number): WorkNode => ({
  id: "area:a",
  type: "area",
  position: { x, y },
  width: 600,
  height: 400,
  data: { title: "A", count: 1 },
});

test("a grid of the selection is the arrange grid at its top-left, in reading order", () => {
  const nodes = [card("c", 300, 200), card("a", 40, 20), card("b", 500, 30)];
  const placed = selectedPlacements(["a", "b", "c"], nodes);
  const to = arrange(placed, "grid");
  const expected = grid(
    [
      { width: 100, height: 60 },
      { width: 100, height: 60 },
      { width: 100, height: 60 },
    ],
    { columns: 2, gap: 20, origin: { x: 40, y: 20 } },
  );
  expect(["a", "b", "c"].map((id) => to.get(id))).toEqual(expected.map(({ x, y }) => ({ x, y })));
});

test("row reads left to right, stack top to bottom, tidy packs around the centre", () => {
  const placed = selectedPlacements(["a", "b"], [card("a", 400, 0), card("b", 0, 300)]);
  expect(arrange(placed, "row")).toEqual(
    new Map([
      ["b", { x: 0, y: 0 }],
      ["a", { x: 120, y: 0 }],
    ]),
  );
  expect(arrange(placed, "stack")).toEqual(
    new Map([
      ["a", { x: 0, y: 0 }],
      ["b", { x: 0, y: 80 }],
    ]),
  );
  const packed = pack([placed[0]!, placed[1]!], { x: 0, y: 0, width: 500, height: 360 }, 20);
  expect(arrange(placed, "tidy")).toEqual(
    new Map([
      ["a", { x: packed[0]!.x, y: packed[0]!.y }],
      ["b", { x: packed[1]!.x, y: packed[1]!.y }],
    ]),
  );
  expect(alignTo(placed, "top").get("b")).toEqual({ x: 0, y: 0 });
});

test("a card inside an area moves relative to it and keeps its parent", () => {
  const nodes = [area(100, 100), card("a", 20, 50, "area:a"), card("b", 0, 0)];
  expect(selectedPlacements(["a"], nodes)[0]).toMatchObject({ x: 120, y: 150 });
  const moved = moveTo(
    nodes,
    new Map([
      ["a", { x: 200, y: 300 }],
      ["b", { x: 0, y: 0 }],
    ]),
  );
  expect(moved[1]).toMatchObject({ parentId: "area:a", position: { x: 100, y: 200 } });
  expect(moved[2]).toBe(nodes[2]);
});

test("fitting an area shrinks it to its members and leaves them where they stand", () => {
  const nodes = [area(0, 0), card("a", 300, 200, "area:a")];
  const fitted = fitArea(nodes, "area:a");
  expect(fitted[0]).toMatchObject({ position: { x: 268, y: 132 }, width: 240, height: 160 });
  expect(selectedPlacements(["a"], fitted)[0]).toMatchObject({ x: 300, y: 200 });
});

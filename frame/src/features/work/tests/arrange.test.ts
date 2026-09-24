import { expect, test } from "vitest";
import { align, bounds, column, distribute, grid, pack, row } from "../lib/arrange";

const card = (width: number, height: number) => ({ width, height });

test("a grid is row-major: columns as wide as their widest card, rows as tall as their tallest", () => {
  expect(
    grid([card(100, 50), card(80, 70), card(120, 40)], {
      columns: 2,
      gap: 10,
      origin: { x: 5, y: 5 },
    }),
  ).toEqual([
    { x: 5, y: 5, width: 100, height: 50 },
    { x: 135, y: 5, width: 80, height: 70 },
    { x: 5, y: 85, width: 120, height: 40 },
  ]);
});

test("row and column are one-line grids", () => {
  const sizes = [card(100, 50), card(60, 30)];
  expect(row(sizes, { gap: 20, origin: { x: 0, y: 0 } }).map((p) => [p.x, p.y])).toEqual([
    [0, 0],
    [120, 0],
  ]);
  expect(column(sizes, { gap: 20, origin: { x: 0, y: 0 } }).map((p) => [p.x, p.y])).toEqual([
    [0, 0],
    [0, 70],
  ]);
});

test("align lines cards up on an edge or a centre line of their bounds", () => {
  const cards = [
    { x: 10, y: 40, width: 100, height: 20 },
    { x: 50, y: 0, width: 40, height: 60 },
  ];
  expect(align(cards, "left").map((p) => p.x)).toEqual([10, 10]);
  expect(align(cards, "top").map((p) => p.y)).toEqual([0, 0]);
  expect(align(cards, "centerX").map((p) => p.x)).toEqual([10, 40]);
  expect(align(cards, "centerY").map((p) => p.y)).toEqual([20, 0]);
});

test("distribute keeps the current order and input order in the result", () => {
  const cards = [
    { x: 300, y: 0, width: 50, height: 10 },
    { x: 0, y: 5, width: 100, height: 10 },
    { x: 120, y: 9, width: 30, height: 10 },
  ];
  expect(distribute(cards, "x", 10)).toEqual([
    { x: 150, y: 0, width: 50, height: 10 },
    { x: 0, y: 5, width: 100, height: 10 },
    { x: 110, y: 9, width: 30, height: 10 },
  ]);
});

test("pack gathers cards into a near-square grid centred on the anchor, in whole pixels", () => {
  const cards = Array.from({ length: 5 }, () => ({ x: 999, y: 999, width: 100, height: 50 }));
  const packed = pack(cards, { x: 0, y: 0, width: 101, height: 101 }, 10);
  expect(packed.map((p) => [p.x, p.y])).toEqual([
    [-109, -4],
    [1, -4],
    [111, -4],
    [-109, 56],
    [1, 56],
  ]);
  expect(bounds(packed)).toEqual({ x: -109, y: -4, width: 320, height: 110 });
  expect(bounds([])).toBeNull();
});

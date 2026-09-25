import { expect, test } from "vitest";
import { reconcileNodes, validScene, validViewport, type CanvasItem } from "../lib/canvas-model";
const item = (id: string): CanvasItem => ({
  id,
  title: id,
  kind: "Resource",
  detail: "Description",
  status: "Ready",
});
test("a semantic update preserves unrelated identity and user geometry", () => {
  const items = [item("one"), item("two")];
  const previous = reconcileNodes([], items);
  previous[0]!.position = { x: 12, y: 27 };
  const next = reconcileNodes(previous, [{ ...items[0]!, status: "Needs review" }, items[1]!]);
  expect(next[1]).toBe(previous[1]);
  expect(next[0]!.position).toEqual({ x: 12, y: 27 });
  expect(next[0]!.ariaLabel).toContain("Needs review");
  expect(reconcileNodes(next, [next[0]!.data, next[1]!.data])).toBe(next);
});
test("rejects duplicate identities, oversized scenes and dangling relationships", () => {
  expect(validScene([item("same"), item("same")], [])).toBe(false);
  expect(
    validScene(
      Array.from({ length: 501 }, (_, i) => item(String(i))),
      [],
    ),
  ).toBe(false);
  expect(
    validScene(
      [item("one")],
      [{ id: "link", source: "one", target: "missing", kind: "reference" }],
    ),
  ).toBe(false);
});
test("invalid persisted geometry cannot reach the canvas", () => {
  expect(validViewport({ x: Infinity, y: 0, zoom: 1 })).toBeUndefined();
  expect(validViewport({ x: 0, y: 0, zoom: 9 })).toBeUndefined();
  const nodes = reconcileNodes([], [item("a")], { a: { x: NaN, y: 1 } });
  expect(nodes[0]!.position).toEqual({ x: 0, y: 0 });
});
test("a large unchanged scene shares all its node objects", () => {
  const items = Array.from({ length: 500 }, (_, i) => item(String(i)));
  const nodes = reconcileNodes([], items);
  expect(reconcileNodes(nodes, items)).toBe(nodes);
});

test("new items do not overlap retained arrangements after another item is removed", () => {
  const previous = reconcileNodes([], [item("a"), item("b")]);
  const next = reconcileNodes(previous, [item("b"), item("c")]);
  expect(next[0]).toBe(previous[1]);
  expect(next[1]!.position).not.toEqual(next[0]!.position);
});

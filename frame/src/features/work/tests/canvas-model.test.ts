import { expect, test } from "vitest";
import {
  applyRemoteView,
  containingArea,
  nodesBounds,
  reconcileNodes,
  sanitizeScene,
  validScene,
  validViewport,
  withClusters,
  type CanvasItem,
} from "../lib/canvas-model";
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
  expect(reconcileNodes(next, [{ ...items[0]!, status: "Needs review" }, items[1]!])).toBe(next);
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
test("a scene the canvas could not draw is repaired instead of hidden", () => {
  const long = { ...item("finding"), title: "t".repeat(900), detail: "d".repeat(5000) };
  const scene = sanitizeScene(
    [long, item("one"), item("one"), { ...item(""), title: "nameless" }],
    [
      { id: "dangling", source: "one", target: "gone", kind: "reference" },
      { id: "self", source: "one", target: "one", kind: "reference" },
      { id: "kept", source: "finding", target: "one", kind: "supports" },
      { id: "kept", source: "one", target: "finding", kind: "supports" },
    ],
  );
  expect(scene.items.map((entry) => entry.id)).toEqual(["finding", "one"]);
  expect(scene.items[0]!.title).toHaveLength(512);
  expect(scene.items[0]!.detail).toHaveLength(2048);
  expect(scene.links.map((link) => [link.id, link.source])).toEqual([["kept", "finding"]]);
  expect(validScene(scene.items, scene.links)).toBe(true);
});
test("invalid persisted geometry cannot reach the canvas", () => {
  expect(validViewport({ x: Infinity, y: 0, zoom: 1 })).toBeUndefined();
  expect(validViewport({ x: 0, y: 0, zoom: 9 })).toBeUndefined();
  const nodes = reconcileNodes([], [item("a")], { a: { x: NaN, y: 1 } });
  expect(nodes[0]!.position).toEqual({ x: 80, y: 120 });
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

test("restored origin remains user-owned while new placements clear chrome", () => {
  const nodes = reconcileNodes([], [item("saved"), item("new")], { saved: { x: 0, y: 0 } });
  expect(nodes[0]!.position).toEqual({ x: 0, y: 0 });
  expect(nodes[1]!.position.y).toBeGreaterThanOrEqual(120);
});

test("areas render first as parents and members keep absolute durable positions", () => {
  const areas = [{ id: "sources", title: "Sources" }];
  const placements = { sources: { x: 100, y: 100, width: 640, height: 420 } };
  const items = [{ ...item("a"), area: "sources" }, item("b")];
  const nodes = reconcileNodes(
    [],
    items,
    { a: { x: 160, y: 180 }, b: { x: 900, y: 40 } },
    {},
    areas,
    placements,
  );
  expect(nodes[0]!.type).toBe("area");
  expect(nodes[0]!.position).toEqual({ x: 100, y: 100 });
  expect(nodes[1]!.parentId).toBe("area:sources");
  expect(nodes[1]!.position).toEqual({ x: 60, y: 80 });
  expect(nodes[2]!.parentId).toBeUndefined();
  expect(containingArea(nodes[1]!, nodes)).toBe("sources");
  expect(containingArea(nodes[2]!, nodes)).toBeNull();
  const moved = reconcileNodes(nodes, [item("a"), item("b")], {}, {}, areas, placements);
  expect(moved[1]!.parentId).toBeUndefined();
  expect(moved[1]!.position).toEqual({ x: 160, y: 180 });
  const remote = applyRemoteView(
    nodes,
    {
      positions: { a: { x: 400, y: 400 }, b: { x: 900, y: 40 } },
      areas: { sources: { x: 200, y: 200, width: 640, height: 420 } },
      viewport: { x: 0, y: 0, zoom: 1 },
    },
    new Set(["a", "b"]),
  );
  expect(remote[0]!.position).toEqual({ x: 200, y: 200 });
  expect(remote[1]!.position).toEqual({ x: 200, y: 200 });
  expect(remote[2]).toBe(nodes[2]);
  expect(nodesBounds(["a", "b"], nodes)).toEqual({ x: 128, y: -28, width: 1084, height: 400 });
});

test("a tall Made group attaches its lines at its first row, and holds an area with no card of its own", () => {
  const nodes = reconcileNodes(
    [],
    ["a", "b", "c"].map(item),
    { a: { x: 40, y: 80 }, b: { x: 40, y: 400 }, c: { x: 40, y: 800 } },
    { a: { width: 200, height: 64 }, b: { width: 200, height: 64 }, c: { width: 200, height: 64 } },
  );
  const clusters = [
    { id: "area", label: "3 parts", more: 0, members: ["a", "b", "c"], inset: 16 },
    { id: "made", label: "Result", more: 0, members: [], within: ["area"], inset: 24 },
  ];
  const kept = sanitizeScene(["a", "b", "c"].map(item), [], clusters).clusters;
  expect(kept.map((cluster) => cluster.id)).toEqual(["area", "made"]);
  const next = withClusters(nodes, kept);
  const made = next.find((node) => node.id === "made")!;
  // Padded and captioned around the area as it would be around a card.
  expect(made.position).toEqual({ x: 40 - 16 - 24, y: 80 - 16 - 20 - 24 - 20 });
  expect(made.height).toBeGreaterThan(240);
  // The first row is the area itself, from the caption down.
  const area = next.find((node) => node.id === "area")!;
  expect((made.data as { anchor?: number }).anchor).toBe(
    Math.round((24 + area.position.y + area.height! - made.position.y) / 2),
  );
  const short = withClusters(nodes.slice(0, 1), [{ ...clusters[0]!, members: ["a"] }]);
  expect((short[0]!.data as { anchor?: number }).anchor).toBeUndefined();
});

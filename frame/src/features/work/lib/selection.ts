import { align, bounds, column, grid, pack, row, type Placement, type Point } from "./arrange";
import { absolutePosition, isItemNode, nodesBounds, type WorkNode } from "./canvas-model";

export type Arrangement = "grid" | "row" | "stack" | "tidy";
export type Alignment = "left" | "top" | "center";
export type PointerTool = "select" | "hand";
const GAP = 20;

type Placed = Placement & { id: string };

/** The selected cards as absolute boxes, in the order they read on the canvas. */
export function selectedPlacements(ids: readonly string[], nodes: readonly WorkNode[]): Placed[] {
  const wanted = new Set(ids);
  return nodes
    .filter((node) => wanted.has(node.id) && isItemNode(node))
    .map((node) => ({
      id: node.id,
      ...absolutePosition(node, nodes),
      width: node.width ?? node.measured?.width ?? 280,
      height: node.height ?? node.measured?.height ?? 160,
    }));
}

const byRows = (a: Placed, b: Placed) => a.y - b.y || a.x - b.x;
const byColumns = (a: Placed, b: Placed) => a.x - b.x || a.y - b.y;

/** Absolute positions for an arrangement anchored where the selection stands now. */
export function arrange(placed: readonly Placed[], how: Arrangement): Map<string, Point> {
  const box = bounds(placed);
  if (!box) return new Map();
  const order = [...placed].sort(how === "row" ? byColumns : byRows);
  const origin = { x: box.x, y: box.y };
  const next =
    how === "grid"
      ? grid(order, { columns: Math.ceil(Math.sqrt(order.length)), gap: GAP, origin })
      : how === "row"
        ? row(order, { gap: GAP, origin })
        : how === "stack"
          ? column(order, { gap: GAP, origin })
          : pack(order, box, GAP);
  return new Map(order.map((p, index) => [p.id, { x: next[index]!.x, y: next[index]!.y }]));
}

export function alignTo(placed: readonly Placed[], how: Alignment): Map<string, Point> {
  const next = align(placed, how === "center" ? "centerX" : how);
  return new Map(placed.map((p, index) => [p.id, { x: next[index]!.x, y: next[index]!.y }]));
}

/** Moves cards to absolute points; a card inside an area keeps its parent. */
export function moveTo(nodes: WorkNode[], to: ReadonlyMap<string, Point>): WorkNode[] {
  if (!to.size) return nodes;
  const byId = new Map(nodes.map((node) => [node.id, node]));
  let changed = false;
  const next = nodes.map((node): WorkNode => {
    const target = to.get(node.id);
    if (!target) return node;
    const parent = node.parentId ? byId.get(node.parentId) : undefined;
    const position = parent
      ? { x: target.x - parent.position.x, y: target.y - parent.position.y }
      : { x: target.x, y: target.y };
    if (position.x === node.position.x && position.y === node.position.y) return node;
    changed = true;
    return { ...node, position };
  });
  return changed ? next : nodes;
}

/** Shrinks an area around its members, 32 px out; the members stay where they stand. */
export function fitArea(nodes: WorkNode[], areaId: string): WorkNode[] {
  const area = nodes.find((node) => node.id === areaId);
  const members = nodes.filter((node) => node.parentId === areaId && isItemNode(node));
  const box = nodesBounds(
    members.map((node) => node.id),
    nodes,
  );
  if (!area || !box) return nodes;
  const same =
    area.position.x === box.x &&
    area.position.y === box.y &&
    area.width === box.width &&
    area.height === box.height;
  if (same) return nodes;
  const dx = area.position.x - box.x;
  const dy = area.position.y - box.y;
  return nodes.map((node): WorkNode => {
    if (node === area)
      return { ...node, position: { x: box.x, y: box.y }, width: box.width, height: box.height };
    if (node.parentId !== areaId) return node;
    return { ...node, position: { x: node.position.x + dx, y: node.position.y + dy } };
  });
}

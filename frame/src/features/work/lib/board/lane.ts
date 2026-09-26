import type { BoardLayout, Rect } from "./layout";

/** A request's lane: its process column at x = 0, its board to the right. */
export const LANE = { column: 300, gutter: 48, stack: 12, between: 96, head: 16 } as const;

type Sized = { id: string; width: number; height: number };
export type LaneParts = {
  /** Request, trail, live pages and sources, top to bottom. */
  column: readonly Sized[];
  /** The board's title and lead, when it has either. */
  head?: Sized;
  board: BoardLayout;
  /** Blocks the person moved, by their offset from the board's corner. */
  pins?: ReadonlyMap<string, { x: number; y: number; width: number; height: number }>;
};
export type LanePlace = {
  rects: Record<string, Rect>;
  /** Where the board's blocks start, under the head. */
  board: Rect;
  /** The lane's top at the board's left edge: a moved block is kept relative to it. */
  corner: { x: number; y: number };
  /** The bottom of whatever the lane holds; the next lane starts 96 px under it. */
  extent: number;
};

export function placeLane(top: number, parts: LaneParts): LanePlace {
  const rects: Record<string, Rect> = {};
  let y = top;
  for (const item of parts.column) {
    rects[item.id] = { x: 0, y, width: item.width, height: item.height };
    y += item.height + LANE.stack;
  }
  const column = y - LANE.stack;
  const x = LANE.column + LANE.gutter;
  let blocks = top;
  if (parts.head) {
    rects[parts.head.id] = { x, y: top, width: parts.head.width, height: parts.head.height };
    blocks = top + parts.head.height + LANE.head;
  }
  for (const [id, rect] of Object.entries(parts.board.at))
    rects[id] = { ...rect, x: rect.x + x, y: rect.y + blocks };
  let extent = Math.max(column, blocks + parts.board.height);
  for (const [id, pin] of parts.pins ?? []) {
    rects[id] = { ...pin, x: x + pin.x, y: top + pin.y };
    extent = Math.max(extent, top + pin.y + pin.height);
  }
  return {
    rects,
    board: { x, y: blocks, width: parts.board.width, height: parts.board.height },
    corner: { x, y: top },
    extent,
  };
}

/** The orb is 24 px; it stands just off the top-right corner of what it works on. */
const MARK = 24;
export function standBeside(rect: Rect): { x: number; y: number } {
  return { x: rect.x + rect.width + 8, y: rect.y - 8 - MARK / 2 };
}

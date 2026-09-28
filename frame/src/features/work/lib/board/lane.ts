import type { BoardLayout, Rect } from "./layout";

/** Bands stand 96 px apart; a result's head sits 16 px over its blocks. */
export const LANE = { between: 96, head: 16 } as const;

type Sized = { id: string; width: number; height: number };
export type LanePlace = {
  rects: Record<string, Rect>;
  /** Where the board's blocks start, under the head. */
  board: Rect;
  /** The lane's top at the board's left edge: a moved block is kept relative to it. */
  corner: { x: number; y: number };
  /** The bottom of whatever the lane holds; the next lane starts 96 px under it. */
  extent: number;
};

/**
 * A request's band, read left to right: the person's words, then one branch
 * per place its runs worked, then the result with its sources under it.
 */
export const BAND = {
  request: 340,
  branch: 548,
  gap: 72,
  stack: 28,
  across: 10,
  sources: 16,
} as const;

export type BandParts = {
  request: Sized;
  branches: readonly Sized[];
  head?: Sized;
  board: BoardLayout;
  sources?: Sized;
  pins?: ReadonlyMap<string, { x: number; y: number; width: number; height: number }>;
};

export function placeBand(top: number, parts: BandParts): LanePlace {
  const rects: Record<string, Rect> = {};
  rects[parts.request.id] = {
    x: 0,
    y: top,
    width: parts.request.width,
    height: parts.request.height,
  };
  let x = BAND.request + BAND.gap;
  // Branches flow into rows as wide as the branch column: sites with a page or
  // two sit side by side instead of each taking a row of their own.
  let branches = top;
  let across = 0;
  let row = 0;
  for (const branch of parts.branches) {
    if (across && across + branch.width > BAND.branch) {
      branches += row + BAND.stack;
      across = 0;
      row = 0;
    }
    rects[branch.id] = { x: x + across, y: branches, width: branch.width, height: branch.height };
    across += branch.width + BAND.across;
    row = Math.max(row, branch.height);
  }
  if (row) branches += row + BAND.stack;
  if (parts.branches.length) x += BAND.branch + BAND.gap;
  let blocks = top;
  if (parts.head) {
    rects[parts.head.id] = { x, y: top, width: parts.head.width, height: parts.head.height };
    blocks = top + parts.head.height + LANE.head;
  }
  for (const [id, rect] of Object.entries(parts.board.at))
    rects[id] = { ...rect, x: rect.x + x, y: rect.y + blocks };
  let bottom = blocks + parts.board.height;
  if (parts.sources) {
    const y = bottom + (parts.board.height ? BAND.sources + 8 : 0);
    rects[parts.sources.id] = { x, y, width: parts.sources.width, height: parts.sources.height };
    bottom = y + parts.sources.height;
  }
  let extent = Math.max(top + parts.request.height, branches - BAND.stack, bottom);
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

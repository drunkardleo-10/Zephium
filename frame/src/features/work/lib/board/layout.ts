import type { Emphasis } from "./types";

export type Rect = { x: number; y: number; width: number; height: number };
/** A block as the layout sees it: how wide it may stand, how tall it is at a width. */
export type LayoutBlock = {
  id: string;
  kind: string;
  emphasis: Emphasis;
  group?: string;
  width: { min: number; ideal: number; max: number };
  height: (width: number) => number;
};
export type BoardLayout = { width: number; height: number; at: Record<string, Rect> };

/**
 * A result is 640–1120 wide, so at 100% it reads whole in the view it opens
 * in; a picture that reads to the right (a diagram, a map) takes the room it
 * draws, up to `reach`, and the run grows to the right for it.
 */
export const BOARD = { min: 640, max: 1120, reach: 2400, gap: 24, row: 24 } as const;

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/**
 * Hero blocks lead, the prose and the other primary blocks follow, the
 * supporting blocks trail; a group's members stand together where its first
 * member stood.
 */
function reading(blocks: readonly LayoutBlock[]): LayoutBlock[] {
  const tier = (block: LayoutBlock) =>
    block.emphasis === "hero" ? 0 : block.emphasis === "primary" ? 1 : 2;
  const ordered = [...blocks].sort((a, b) => tier(a) - tier(b));
  const out: LayoutBlock[] = [];
  for (const block of ordered) {
    if (out.includes(block)) continue;
    out.push(block);
    if (block.group)
      for (const mate of ordered)
        if (mate !== block && mate.group === block.group && !out.includes(mate)) out.push(mate);
  }
  return out;
}

/** A hero spans the board; prose leading keeps its measure and may take an aside. */
const spans = (block: LayoutBlock) => block.emphasis === "hero" && block.kind !== "prose";

/** Rows at a board width: what may stand beside what, and nothing wider than the board. */
function pack(blocks: readonly LayoutBlock[], width: number, open?: string): LayoutBlock[][] {
  const rows: LayoutBlock[][] = [];
  let row: LayoutBlock[] = [];
  const flush = () => {
    if (row.length) rows.push(row);
    row = [];
  };
  const min = (block: LayoutBlock) => Math.min(block.width.min, width);
  for (const block of blocks) {
    const alone = spans(block) || block.id === open || min(block) > (width - BOARD.gap) / 2;
    if (alone) {
      flush();
      rows.push([block]);
      continue;
    }
    const supporting = block.emphasis === "supporting";
    const cap = supporting ? 3 : 2;
    const head = row[0];
    // A primary block left alone in its row takes the supporting blocks that fit beside it.
    const joins =
      supporting &&
      row.length >= 1 &&
      row.every((entry) => entry.emphasis === "supporting" || entry === head) &&
      head?.emphasis === "primary";
    const fits =
      !!head &&
      ((head.emphasis === "supporting") === supporting || joins) &&
      row.length < cap &&
      row.reduce((sum, entry) => sum + min(entry) + BOARD.gap, 0) + min(block) <= width &&
      // A group starts its own row and keeps it to itself.
      (block.group ?? "") === (head.group ?? "");
    if (!fits) flush();
    row.push(block);
  }
  flush();
  return rows;
}

/** Widths in a row: each its ideal, shrunk toward its least or grown toward its most to fit. */
function widths(row: readonly LayoutBlock[], width: number): number[] {
  const room = width - BOARD.gap * (row.length - 1);
  const least = row.map((block) => Math.min(block.width.min, room));
  const most = row.map((block, index) => Math.max(least[index]!, Math.min(block.width.max, room)));
  const out = row.map((block, index) => clamp(block.width.ideal, least[index]!, most[index]!));
  let sum = out.reduce((total, value) => total + value, 0);
  if (sum > room) {
    const slack = out.reduce((total, value, index) => total + value - least[index]!, 0);
    const cut = Math.min(1, (sum - room) / Math.max(1, slack));
    out.forEach((value, index) => (out[index] = value - (value - least[index]!) * cut));
  } else if (row.length > 1 && sum < room) {
    const slack = out.reduce((total, value, index) => total + most[index]! - value, 0);
    const give = Math.min(1, (room - sum) / Math.max(1, slack));
    out.forEach((value, index) => (out[index] = value + (most[index]! - value) * give));
  }
  const floored = out.map(Math.floor);
  sum = floored.reduce((total, value) => total + value, 0);
  // Rounding never pushes a row past the board.
  if (sum > room) floored[floored.length - 1]! -= sum - room;
  return floored;
}

/**
 * A board laid out for its content: a width between 640 and 1120 that its
 * widest row asks for (wider only for a spanning picture that draws wider), rows top to bottom, nothing overlapping. An open block
 * takes a row of its own at the board's width; a pinned block is the person's
 * and takes no room.
 */
export function boardLayout(
  blocks: readonly LayoutBlock[],
  { open, pinned = new Set() }: { open?: string; pinned?: ReadonlySet<string> } = {},
): BoardLayout {
  const laid = reading(blocks.filter((block) => !pinned.has(block.id)));
  if (!laid.length) return { width: BOARD.min, height: 0, at: {} };
  const wide = Math.max(0, ...laid.map((block) => (spans(block) ? block.width.ideal : 0)));
  const cap = clamp(wide, BOARD.max, BOARD.reach);
  const asked = pack(laid, cap).reduce((need, row) => {
    const ideal = row.reduce(
      (sum, block, index) => sum + Math.min(block.width.ideal, cap) + (index ? BOARD.gap : 0),
      0,
    );
    return Math.max(need, ideal);
  }, 0);
  const width = Math.round(clamp(asked, BOARD.min, cap));
  const at: Record<string, Rect> = {};
  let y = 0;
  for (const row of pack(laid, width, open)) {
    const whole = row.length === 1 && (row[0]!.id === open || spans(row[0]!));
    const sizes = whole ? [width] : widths(row, width);
    const heights = row.map((block, index) => Math.max(1, Math.ceil(block.height(sizes[index]!))));
    // A row's blocks share its height, so their surfaces end together.
    const tallest = Math.max(...heights);
    let x = 0;
    row.forEach((block, index) => {
      const w = sizes[index]!;
      at[block.id] = { x, y, width: w, height: tallest };
      x += w + BOARD.gap;
    });
    y += tallest + BOARD.row;
  }
  return { width, height: Math.max(0, y - BOARD.row), at };
}

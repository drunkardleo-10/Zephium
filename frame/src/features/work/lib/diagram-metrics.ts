import type { CanvasSize } from "./canvas-model";

/**
 * A diagram reads top to bottom: a part's card, the air beside it in a row and
 * between rows, the most parts a row holds before it wraps, the column its
 * tiers are named in, the extra air above each tier, and the widest it draws.
 */
export const DIAGRAM = {
  node: { width: 176, height: 72 },
  column: 20,
  row: 56,
  across: 5,
  gutter: 88,
  tier: 16,
  /** The widest a picture draws at full size: a result's width. */
  reach: 1120,
} as const satisfies Record<string, number | CanvasSize>;

/** A flow's name on its line: caption characters at about 6.6 px, padded, up to two lines. */
export const PLATE = { char: 6.6, pad: 12, line: 15, height: 18, max: 180 } as const;
const plateRun = (label: string) => Math.ceil(label.trim().length * PLATE.char + PLATE.pad);
export const plateWidth = (label: string) => Math.min(PLATE.max, plateRun(label));
export const plateHeight = (label: string) =>
  plateRun(label) > PLATE.max ? PLATE.height + PLATE.line : PLATE.height;

/**
 * The column a diagram's tiers are named in: wide enough for the longest word
 * of any tier's name in small capitals (names wrap between words), none
 * without tiers.
 */
export function gutterOf(lanes: readonly string[]): number {
  if (!lanes.length) return 0;
  const longest = Math.max(0, ...lanes.flatMap((name) => name.split(/\s+/u).map((w) => w.length)));
  return Math.min(168, Math.max(DIAGRAM.gutter, Math.ceil(longest * 7.8) + 36));
}

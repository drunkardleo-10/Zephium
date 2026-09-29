/**
 * A part's own node: its name in a 144 px column, then its work. A browser
 * part's pages stand as small browser windows, the live one (or the last one
 * read) in front and up to two behind it, at one size whether the part works
 * or is done, so a run never re-lays out when a part ends.
 */
export const PART = {
  label: 144,
  gap: 24,
  /** A page's window: a slim bar over its frame at 16:10. */
  window: 336,
  bar: 28,
  frame: 210,
  /** Each window behind the front one sits this far right and down. */
  behindX: 14,
  behindY: 10,
  shown: 3,
  sources: 280,
  sourceRow: 28,
  sourceRows: 3,
  helper: 400,
  helperLines: 5,
  ask: 360,
  /** Until an ask card has measured itself: a Confirm carries the page it will change. */
  askConfirm: 432,
  askHeight: 176,
  /** The name and one line under it. */
  labelHeight: 48,
} as const;

/** Where a part's row reports the height its name and its work take. */
export const rowKey = (part: string) => `${part}:row`;

export type PartShape =
  | { kind: "pages"; count: number }
  | { kind: "sources"; rows: number; more: boolean }
  | { kind: "helper"; rows: number }
  | { kind: "ask"; confirm: boolean }
  | { kind: "label" };

/** Where the part's work starts, right of its name. */
export const partLead = PART.label + PART.gap;

/** The windows of a part's pages: the front one and up to two behind it. */
export function stackSize(count: number) {
  const behind = Math.min(PART.shown, Math.max(1, count)) - 1;
  return {
    width: PART.window + behind * PART.behindX,
    height: PART.bar + PART.frame + behind * PART.behindY,
  };
}

/**
 * A part's size: its work's width right of its name, and the taller of its
 * name and its work, as the row last measured them or as estimated until then.
 */
export function partSize(shape: PartShape, measured?: number): { width: number; height: number } {
  const lead = partLead;
  const tall = (estimate: number) => Math.max(PART.labelHeight, measured ?? estimate);
  switch (shape.kind) {
    case "pages": {
      const stack = stackSize(shape.count);
      return { width: lead + stack.width, height: Math.max(stack.height, measured ?? 0) };
    }
    case "sources":
      return {
        width: lead + PART.sources,
        height: tall(
          Math.min(PART.sourceRows, shape.rows) * PART.sourceRow + (shape.more ? 24 : 0),
        ),
      };
    case "helper":
      return { width: lead + PART.helper, height: tall(shape.rows * 24 + 8) };
    case "ask":
      return {
        width: lead + PART.ask,
        height: tall(shape.confirm ? PART.askConfirm : PART.askHeight),
      };
    case "label":
      return { width: 240, height: tall(56) };
  }
}

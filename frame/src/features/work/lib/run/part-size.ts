import type { Detail } from "../board/types";
/**
 * A part's own node: its name in a 120 px column, then its pages. While the
 * part works they stand as frames, up to three, the live one first; once it
 * is done they fold into a stack at the start of the row.
 */
export const PART = {
  label: 120,
  gap: 16,
  tile: 176,
  tileGap: 12,
  /** A frame at 16:10 with its caption, up to two lines, under it. */
  tileHeight: 110 + 6 + 32,
  shown: 3,
  /** Folded pages: 144 × 90 thumbnails, each one behind 12 across and 11 down. */
  thumb: 144,
  thumbHeight: 90,
  stackStep: 12,
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

/** Where a part's ask card reports its height. */
export const askKey = (part: string) => `${part}:ask`;
/** Where a helper's own view of its part reports its height. */
export const contentKey = (part: string) => `${part}:content`;

export type PartShape =
  | { kind: "frames"; count: number }
  | { kind: "stack"; count: number }
  | { kind: "sources"; rows: number; more: boolean }
  | { kind: "helper"; rows: number; height?: number }
  | { kind: "ask"; height: number }
  | { kind: "label" };

/** The name's column: surveyed from afar it widens, so a name reads whole at its large size. */
export const labelWidth = (detail: Detail = "full") => (detail === "full" ? PART.label : 200);

export function partSize(
  shape: PartShape,
  detail: Detail = "full",
): { width: number; height: number } {
  const lead = labelWidth(detail) + PART.gap;
  switch (shape.kind) {
    case "frames": {
      const count = Math.max(1, Math.min(PART.shown, shape.count));
      return {
        width: lead + count * PART.tile + (count - 1) * PART.tileGap,
        height: PART.tileHeight,
      };
    }
    case "stack": {
      const behind = Math.min(PART.shown, Math.max(1, shape.count)) - 1;
      return {
        width: lead + PART.thumb + behind * PART.stackStep,
        height: PART.thumbHeight + behind * 11,
      };
    }
    case "sources":
      return {
        width: lead + PART.sources,
        height: Math.max(
          PART.labelHeight,
          Math.min(PART.sourceRows, shape.rows) * PART.sourceRow + (shape.more ? 24 : 0),
        ),
      };
    case "helper":
      // A helper's own view stands 400 wide, a 24 px row per thing it did, until it measures itself.
      return {
        width: lead + PART.helper,
        height: Math.max(PART.labelHeight, shape.height ?? shape.rows * 24 + 8),
      };
    case "ask":
      return { width: lead + PART.ask, height: Math.max(PART.labelHeight, shape.height) };
    case "label":
      return { width: Math.max(320, lead + 200), height: 56 };
  }
}

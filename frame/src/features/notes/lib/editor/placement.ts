export type Box = { left: number; top: number; right: number; bottom: number };

export type Placement = {
  left: number;
  top: number;
  side: "above" | "below";
  /** Set when neither side has room for the whole of it. */
  maxHeight: number | null;
};

const MARGIN = 8;

/** Where a floating layer may draw: its notes area, inset a little, never
 *  past the window. In the sidebar that area ends where the page's native
 *  view begins, and anything drawn beyond it is hidden underneath. */
export function boundsOf(anchor: Element): Box {
  const area = anchor.closest("[data-note-bounds]")?.getBoundingClientRect();
  const view = { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight };
  return {
    left: Math.max(area?.left ?? view.left, view.left) + MARGIN,
    top: Math.max(area?.top ?? view.top, view.top) + MARGIN,
    right: Math.min(area?.right ?? view.right, view.right) - MARGIN,
    bottom: Math.min(area?.bottom ?? view.bottom, view.bottom) - MARGIN,
  };
}

/** Places a layer of `size` beside `anchor`, on the `prefer`red side when it
 *  fits there, on the other when only that one fits, and otherwise on the
 *  roomier side, cut to what it has. */
export function place(
  anchor: Box,
  size: { width: number; height: number },
  bounds: Box,
  prefer: "above" | "below",
  { gap = 6, align = "start" }: { gap?: number; align?: "start" | "center" | "end" } = {},
): Placement {
  const room = { above: anchor.top - gap - bounds.top, below: bounds.bottom - anchor.bottom - gap };
  const other = prefer === "above" ? "below" : "above";
  const side =
    room[prefer] >= size.height
      ? prefer
      : room[other] >= size.height
        ? other
        : room.above > room.below
          ? "above"
          : "below";
  const height = Math.min(size.height, Math.max(room[side], 0));
  const start =
    align === "center"
      ? (anchor.left + anchor.right - size.width) / 2
      : align === "end"
        ? anchor.right - size.width
        : anchor.left;
  return {
    left: Math.round(Math.max(bounds.left, Math.min(start, bounds.right - size.width))),
    top: Math.round(side === "above" ? anchor.top - gap - height : anchor.bottom + gap),
    side,
    maxHeight: height < size.height ? Math.floor(height) : null,
  };
}

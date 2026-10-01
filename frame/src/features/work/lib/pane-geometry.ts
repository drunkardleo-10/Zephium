export type PaneRect = { x: number; y: number; width: number; height: number };

/** Session-only: the last pane rect the user settled on, never canvas state. */
let remembered: PaneRect | null = null;

export function remember(rect: PaneRect) {
  remembered = rect;
}

/** Everything opens in the centre: a page stands over the middle of the canvas, most of it. */
export function paneGeometry(bounds: DOMRect): PaneRect {
  const inset = 16;
  const placed = over;
  over = null;
  if (placed) {
    const width = Math.min(placed.width, bounds.width);
    const height = Math.min(placed.height, bounds.height);
    return {
      x: Math.min(Math.max(placed.x, bounds.left), bounds.right - width),
      y: Math.min(Math.max(placed.y, bounds.top), bounds.bottom - height),
      width,
      height,
    };
  }
  const width = Math.min(Math.max(640, Math.round(bounds.width * 0.8)), bounds.width - inset * 2);
  const height = Math.min(Math.round(bounds.height * 0.86), bounds.height - inset * 2);
  const fallback = {
    x: Math.round(bounds.left + (bounds.width - width) / 2),
    y: Math.round(bounds.top + (bounds.height - height) / 2),
    width,
    height,
  };
  const last = remembered;
  if (!last) return fallback;
  const w = Math.min(last.width, bounds.width - inset * 2);
  const h = Math.min(last.height, bounds.height - inset * 2);
  return {
    x: Math.min(Math.max(last.x, bounds.left), bounds.right - w),
    y: Math.min(Math.max(last.y, bounds.top), bounds.bottom - h),
    width: w,
    height: h,
  };
}

/** The pane's own floor; a pane opened over a small card grows to it. */
const PANE_MIN = { width: 482, height: 366 };
let over: PaneRect | null = null;

/** The next pane opens over this card, centred on it, at least the pane's minimum. */
export function openOver(card: DOMRect) {
  const width = Math.max(PANE_MIN.width, Math.round(card.width));
  const height = Math.max(PANE_MIN.height, Math.round(card.height));
  over = {
    x: Math.round(card.left + card.width / 2 - width / 2),
    y: Math.round(card.top + card.height / 2 - height / 2),
    width,
    height,
  };
}

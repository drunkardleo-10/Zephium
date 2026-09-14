export type PaneRect = { x: number; y: number; width: number; height: number };

/** Session-only: the last pane rect the user settled on, never canvas state. */
let remembered: PaneRect | null = null;

export function remember(rect: PaneRect) {
  remembered = rect;
}

/** Launcher-like default: a tall pane hugging the canvas's trailing edge. */
export function paneGeometry(bounds: DOMRect): PaneRect {
  const inset = 16;
  const width = Math.min(Math.max(640, Math.round(bounds.width * 0.56)), bounds.width - inset * 2);
  const height = bounds.height - inset * 2;
  const fallback = {
    x: bounds.right - width - inset,
    y: bounds.top + inset,
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

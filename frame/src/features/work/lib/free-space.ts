type Rect = { x: number; y: number; width: number; height: number };
type Point = { x: number; y: number };

const STEP = 24;
const RINGS = 40;

const clear = (a: Rect, taken: readonly Rect[], gap: number) =>
  taken.every(
    (b) =>
      a.x + a.width + gap <= b.x ||
      b.x + b.width + gap <= a.x ||
      a.y + a.height + gap <= b.y ||
      b.y + b.height + gap <= a.y,
  );

/**
 * The nearest place to `want` (the thing's centre) where a thing of `size`
 * touches nothing taken, `gap` apart, on an 8 px grid: rings outward on a
 * 24 px step, the nearest free spot of the first ring that has one. Where
 * nothing is free nearby, the spot just under everything taken.
 */
export function freeSpot(
  want: Point,
  size: { width: number; height: number },
  taken: readonly Rect[],
  gap = 32,
): Point {
  const snap = (value: number) => Math.round(value / 8) * 8;
  const at = (x: number, y: number): Rect => ({
    x: snap(x - size.width / 2),
    y: snap(y - size.height / 2),
    ...size,
  });
  const centre = (rect: Rect) => ({ x: rect.x + size.width / 2, y: rect.y + size.height / 2 });
  if (clear(at(want.x, want.y), taken, gap)) return centre(at(want.x, want.y));
  for (let ring = 1; ring <= RINGS; ring += 1) {
    let best: { rect: Rect; distance: number } | null = null;
    for (let i = -ring; i <= ring; i += 1)
      for (const [dx, dy] of [
        [i, -ring],
        [i, ring],
        [-ring, i],
        [ring, i],
      ] as const) {
        const rect = at(want.x + dx * STEP, want.y + dy * STEP);
        if (!clear(rect, taken, gap)) continue;
        const distance = Math.hypot(dx, dy);
        if (!best || distance < best.distance) best = { rect, distance };
      }
    if (best) return centre(best.rect);
  }
  const bottom = Math.max(want.y, ...taken.map((rect) => rect.y + rect.height));
  return centre(at(want.x, bottom + gap + size.height / 2));
}

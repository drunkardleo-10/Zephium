/** Canvas geometry both the run and the person arrange with. Pure, integer, input-ordered. */
export type Size = { width: number; height: number };
export type Point = { x: number; y: number };
export type Placement = Point & Size;
export type Edge = "left" | "top" | "centerX" | "centerY";
export type Axis = "x" | "y";

const whole = (placement: Placement): Placement => ({
  x: Math.round(placement.x),
  y: Math.round(placement.y),
  width: Math.round(placement.width),
  height: Math.round(placement.height),
});

/** Row-major cells: a column is as wide as its widest card, a row as tall as its tallest. */
export function grid(
  sizes: readonly Size[],
  { columns, gap, origin }: { columns: number; gap: number; origin: Point },
): Placement[] {
  const across = Math.max(1, Math.floor(columns));
  const widths: number[] = [];
  const heights: number[] = [];
  sizes.forEach((size, index) => {
    const column = index % across;
    const row = Math.floor(index / across);
    widths[column] = Math.max(widths[column] ?? 0, size.width);
    heights[row] = Math.max(heights[row] ?? 0, size.height);
  });
  const offset = (lengths: number[], upto: number) =>
    lengths.slice(0, upto).reduce((sum, length) => sum + length + gap, 0);
  return sizes.map((size, index) =>
    whole({
      x: origin.x + offset(widths, index % across),
      y: origin.y + offset(heights, Math.floor(index / across)),
      width: size.width,
      height: size.height,
    }),
  );
}

export function row(sizes: readonly Size[], options: { gap: number; origin: Point }): Placement[] {
  return grid(sizes, { ...options, columns: Math.max(1, sizes.length) });
}

export function column(
  sizes: readonly Size[],
  options: { gap: number; origin: Point },
): Placement[] {
  return grid(sizes, { ...options, columns: 1 });
}

export function bounds(placements: readonly Placement[]): Placement | null {
  if (!placements.length) return null;
  const left = Math.min(...placements.map((p) => p.x));
  const top = Math.min(...placements.map((p) => p.y));
  const right = Math.max(...placements.map((p) => p.x + p.width));
  const bottom = Math.max(...placements.map((p) => p.y + p.height));
  return whole({ x: left, y: top, width: right - left, height: bottom - top });
}

/** Lines every card up on one edge or centre line of the group's bounds. */
export function align(placements: readonly Placement[], edge: Edge): Placement[] {
  const box = bounds(placements);
  if (!box) return [];
  return placements.map((p) =>
    whole(
      edge === "left"
        ? { ...p, x: box.x }
        : edge === "top"
          ? { ...p, y: box.y }
          : edge === "centerX"
            ? { ...p, x: box.x + (box.width - p.width) / 2 }
            : { ...p, y: box.y + (box.height - p.height) / 2 },
    ),
  );
}

/** Even air between cards along an axis, in their current order, from the first one's start. */
export function distribute(placements: readonly Placement[], axis: Axis, gap: number): Placement[] {
  const length = axis === "x" ? "width" : "height";
  const order = placements
    .map((placement, index) => ({ placement, index }))
    .sort((a, b) => a.placement[axis] - b.placement[axis] || a.index - b.index);
  const out: Placement[] = [];
  let cursor = order[0]?.placement[axis] ?? 0;
  for (const { placement, index } of order) {
    out[index] = whole({ ...placement, [axis]: cursor });
    cursor += placement[length] + gap;
  }
  return out;
}

/** A near-square grid of the cards, centred on the anchor. */
export function pack(
  placements: readonly Placement[],
  around: Placement,
  gap: number,
): Placement[] {
  if (!placements.length) return [];
  const columns = Math.ceil(Math.sqrt(placements.length));
  const packed = grid(placements, { columns, gap, origin: { x: 0, y: 0 } });
  const box = bounds(packed)!;
  const dx = around.x + (around.width - box.width) / 2;
  const dy = around.y + (around.height - box.height) / 2;
  return packed.map((p) => whole({ ...p, x: p.x + dx, y: p.y + dy }));
}

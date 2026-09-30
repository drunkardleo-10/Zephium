export type Point = { x: number; y: number };

/** Elbows turn on a 12 px radius. */
const ELBOW = 12;

const diagonal = (a: Point, b: Point) => a.x !== b.x && a.y !== b.y;

/**
 * A polyline as an SVG path: orthogonal elbows turn on a 12 px radius, and a
 * diagonal stretch is drawn as a curve that leaves and lands level, the way a
 * branch leaves its stem. A corner takes the whole of a segment that ends the
 * line and half of one it shares with another corner.
 */
export function roundedPath(points: readonly Point[], radius = ELBOW): string {
  const list = points.filter(
    (point, index) =>
      index === 0 || point.x !== points[index - 1]!.x || point.y !== points[index - 1]!.y,
  );
  if (!list.length) return "";
  const first = list[0]!;
  let d = `M ${first.x},${first.y}`;
  for (let index = 1; index < list.length; index++) {
    const prev = list[index - 1]!;
    const at = list[index]!;
    const next = list[index + 1];
    if (diagonal(prev, at)) {
      const half = (at.x - prev.x) / 2;
      d += ` C ${prev.x + half},${prev.y} ${at.x - half},${at.y} ${at.x},${at.y}`;
      continue;
    }
    const inLength = Math.hypot(at.x - prev.x, at.y - prev.y);
    const outLength = next ? Math.hypot(next.x - at.x, next.y - at.y) : 0;
    const r = next
      ? Math.min(
          radius,
          index === 1 || diagonal(list[index - 2] ?? prev, prev) ? inLength : inLength / 2,
          index === list.length - 2 ? outLength : outLength / 2,
        )
      : 0;
    const straight =
      !next ||
      diagonal(at, next) ||
      (prev.x === at.x && at.x === next.x) ||
      (prev.y === at.y && at.y === next.y) ||
      r <= 0;
    if (straight) {
      d += ` L ${at.x},${at.y}`;
      continue;
    }
    const a = {
      x: at.x - ((at.x - prev.x) / inLength) * r,
      y: at.y - ((at.y - prev.y) / inLength) * r,
    };
    const b = {
      x: at.x + ((next.x - at.x) / outLength) * r,
      y: at.y + ((next.y - at.y) / outLength) * r,
    };
    d += ` L ${a.x},${a.y} Q ${at.x},${at.y} ${b.x},${b.y}`;
  }
  return d;
}

/**
 * Lines from one point to several ends, as branches from one stem: the end
 * level with it straight across, every other one curving away from the same
 * point, up or down, so parallel work forks symmetrically.
 */
export function branch(start: Point, ends: readonly Point[]): Point[][] {
  return ends.map((end) => [start, end]);
}

/**
 * Lines from several starts into one end: each runs level to the collector,
 * then curves in, the mirror of a fork.
 */
export function merge(starts: readonly Point[], collector: number, end: Point): Point[][] {
  return starts.map((start) =>
    start.y === end.y || start.x >= collector
      ? [start, end]
      : [start, { x: collector, y: start.y }, end],
  );
}

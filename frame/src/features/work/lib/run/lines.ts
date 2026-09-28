export type Point = { x: number; y: number };

/** Elbows turn on a 12 px radius. */
const ELBOW = 12;

/**
 * An orthogonal polyline as an SVG path with rounded elbows. A corner takes
 * the whole of a segment that ends the line and half of one it shares with
 * another corner, so a short bus stub still turns on its full radius.
 */
export function roundedPath(points: readonly Point[], radius = ELBOW): string {
  const list = points.filter(
    (point, index) =>
      index === 0 || point.x !== points[index - 1]!.x || point.y !== points[index - 1]!.y,
  );
  if (!list.length) return "";
  const first = list[0]!;
  let d = `M ${first.x},${first.y}`;
  for (let index = 1; index < list.length - 1; index++) {
    const prev = list[index - 1]!;
    const at = list[index]!;
    const next = list[index + 1]!;
    const inLength = Math.hypot(at.x - prev.x, at.y - prev.y);
    const outLength = Math.hypot(next.x - at.x, next.y - at.y);
    const r = Math.min(
      radius,
      index === 1 ? inLength : inLength / 2,
      index === list.length - 2 ? outLength : outLength / 2,
    );
    const straight =
      (prev.x === at.x && at.x === next.x) || (prev.y === at.y && at.y === next.y) || r <= 0;
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
  const last = list.at(-1)!;
  if (list.length > 1) d += ` L ${last.x},${last.y}`;
  return d;
}

/**
 * Lines from one point to several ends stacked below it, as a bus: the first
 * end straight across, every later one taking the trunk down from where the
 * one above it turned off, so no two lines ever draw the same pixels.
 */
export function fanOut(start: Point, trunk: number, ends: readonly Point[]): Point[][] {
  const sorted = ends.map((end, index) => ({ end, index })).sort((a, b) => a.end.y - b.end.y);
  const out: Point[][] = [];
  // Where the next line takes the trunk from: the start, or where the one above turned off.
  let joint: Point | undefined;
  for (const { end, index } of sorted) {
    const from: Point | undefined = joint;
    let route: Point[];
    if (!from)
      route =
        end.y === start.y
          ? [start, end]
          : [start, { x: trunk, y: start.y }, { x: trunk, y: end.y }, end];
    else if (from.x !== trunk)
      // The line above ran straight: this one turns off it an elbow before the trunk.
      route = [from, { x: trunk, y: from.y }, { x: trunk, y: end.y }, end];
    else route = [from, { x: trunk, y: end.y }, end];
    joint =
      route.length === 2
        ? { x: trunk - ELBOW, y: end.y }
        : { x: trunk, y: Math.max(from?.y ?? start.y, end.y - ELBOW) };
    out[index] = route;
  }
  return out;
}

/**
 * Lines from several starts stacked below one end, merging on a collector
 * before it: the mirror of `fanOut`, each line joining the one above it.
 */
export function fanIn(starts: readonly Point[], collector: number, end: Point): Point[][] {
  return fanOut(
    { x: -end.x, y: end.y },
    -collector,
    starts.map((start) => ({ x: -start.x, y: start.y })),
  ).map((route) => route.map((point) => ({ x: -point.x, y: point.y })).reverse());
}

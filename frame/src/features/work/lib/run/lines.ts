export type Point = { x: number; y: number };

/** Elbows turn on a 12 px radius. */
const ELBOW = 12;

/**
 * An orthogonal polyline as an SVG path with rounded elbows. A corner takes
 * the whole of a segment that ends the line and half of one it shares with
 * another corner, so a short stub still turns on its full radius.
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
 * Lines from one point to several ends, as a fork: level ends straight
 * across, the rest up or down a trunk from the same branch point, each side
 * a bus whose later lines leave the trunk where the one before turned away,
 * so no two lines ever draw the same pixels.
 */
export function branch(start: Point, trunk: number, ends: readonly Point[]): Point[][] {
  const out: Point[][] = [];
  let stub = false;
  ends.forEach((end, index) => {
    if (end.y !== start.y) return;
    out[index] = [start, end];
    stub = true;
  });
  for (const direction of [-1, 1]) {
    const side = ends
      .map((end, index) => ({ end, index }))
      .filter(({ end }) => Math.sign(end.y - start.y) === direction)
      .sort((a, b) => direction * (a.end.y - b.end.y));
    let joint: Point | undefined;
    for (const { end, index } of side) {
      if (!joint) {
        const from = stub ? { x: trunk - ELBOW, y: start.y } : start;
        out[index] = [from, { x: trunk, y: start.y }, { x: trunk, y: end.y }, end];
        stub = true;
      } else out[index] = [joint, { x: trunk, y: end.y }, end];
      joint = { x: trunk, y: end.y - direction * ELBOW };
    }
  }
  return out;
}

/**
 * Lines from several starts into one end: the mirror of a fork, each line
 * running level to the collector and joining the trunk into the end.
 */
export function merge(starts: readonly Point[], collector: number, end: Point): Point[][] {
  return branch(
    { x: -end.x, y: end.y },
    -collector,
    starts.map((start) => ({ x: -start.x, y: start.y })),
  ).map((route) => route.map((point) => ({ x: -point.x, y: point.y })).reverse());
}

import type { CanvasPosition } from "./canvas-model";
import type { DiagramLayout, DiagramShape } from "./diagram";
import { DIAGRAM, plateHeight, plateWidth } from "./diagram-metrics";

type Box = { x: number; y: number; width: number; height: number };
type Segment = { flow: number; a: CanvasPosition; b: CanvasPosition; vertical: boolean };
export type Plate = { at: CanvasPosition; resting: boolean };

const EPSILON = 0.75;
/** Air a name keeps from a part, from another name, and from the ends of its run. */
const AIR = { part: 4, plate: 6, end: 6 } as const;
/** Where along a run a name tries to stand, from its middle outwards. */
const ALONG = [0.5, 0.36, 0.64, 0.22, 0.78];

const segmentsOf = (flow: number, points: readonly CanvasPosition[]): Segment[] =>
  points.slice(1).map((b, index) => {
    const a = points[index]!;
    return { flow, a, b, vertical: Math.abs(a.x - b.x) < EPSILON };
  });

const span = (segment: Segment) =>
  segment.vertical
    ? [Math.min(segment.a.y, segment.b.y), Math.max(segment.a.y, segment.b.y)]
    : [Math.min(segment.a.x, segment.b.x), Math.max(segment.a.x, segment.b.x)];

/** Two runs on one line over more than a few pixels: a trunk they share. */
function overlaps(one: Segment, other: Segment): boolean {
  if (one.vertical !== other.vertical) return false;
  const across = one.vertical ? one.a.x - other.a.x : one.a.y - other.a.y;
  if (Math.abs(across) > EPSILON) return false;
  const [a0, a1] = span(one);
  const [b0, b1] = span(other);
  return Math.min(a1!, b1!) - Math.max(a0!, b0!) > 2;
}

const meets = (box: Box, other: Box, air: number) =>
  box.x < other.x + other.width + air &&
  other.x < box.x + box.width + air &&
  box.y < other.y + other.height + air &&
  other.y < box.y + box.height + air;

/** Whether a run passes through a box (touching its edge is not passing). */
function crosses(segment: Segment, box: Box): boolean {
  if (segment.vertical) {
    const [lo, hi] = span(segment);
    return (
      segment.a.x > box.x + 0.5 &&
      segment.a.x < box.x + box.width - 0.5 &&
      hi! > box.y + 0.5 &&
      lo! < box.y + box.height - 0.5
    );
  }
  const [lo, hi] = span(segment);
  return (
    segment.a.y > box.y + 0.5 &&
    segment.a.y < box.y + box.height - 0.5 &&
    hi! > box.x + 0.5 &&
    lo! < box.x + box.width - 0.5
  );
}

/**
 * Where each named flow's name stands: on a run of its own line that no other
 * flow shares (the branch into its target where a flow fans out, the stem
 * from its source where flows gather), clear of every part, every other name
 * and every other line. A name that finds no such place still has one it
 * shows at when its part is looked at, but does not stand at rest. Flows that
 * share a run and a name show that name once, on the shared run.
 */
export function placePlates(shape: DiagramShape, layout: DiagramLayout): Map<number, Plate> {
  const { width: W, height: H } = DIAGRAM.node;
  const parts: Box[] = Object.values(layout.at).map((at) => ({ ...at, width: W, height: H }));
  // The other way of a pair reads in its part's popover; its line is its partner's.
  const flows = shape.flows.filter(
    (flow) => layout.flows[flow.index] && !layout.flows[flow.index]!.twin,
  );
  const lines = new Map(
    flows.map((flow) => [flow.index, segmentsOf(flow.index, layout.flows[flow.index]!.points)]),
  );
  const all = [...lines.values()].flat();
  const start = (index: number) => layout.flows[index]!.points[0]!;
  const end = (index: number) => layout.flows[index]!.points.at(-1)!;
  const near = (a: CanvasPosition, b: CanvasPosition) =>
    Math.abs(a.x - b.x) < EPSILON && Math.abs(a.y - b.y) < EPSILON;
  const placed: Box[] = [];
  const out = new Map<number, Plate>();

  /** Those flows a run is shared with; the run is the name's own when every one is named alike. */
  const sharers = (segment: Segment) =>
    all.filter((other) => other.flow !== segment.flow && overlaps(segment, other));

  function candidates(index: number, label: string): Segment[] {
    const own = lines.get(index)!;
    const alike = (segment: Segment) =>
      sharers(segment).every(
        (other) => shape.flows.find((flow) => flow.index === other.flow)?.label === label,
      );
    const free = own.filter((segment) => !sharers(segment).length);
    const shared = own.filter((segment) => sharers(segment).length && alike(segment));
    const fanOut = flows.some(
      (flow) => flow.index !== index && near(start(flow.index), start(index)),
    );
    const fanIn = flows.some((flow) => flow.index !== index && near(end(flow.index), end(index)));
    const length = (segment: Segment) => {
      const [lo, hi] = span(segment);
      return hi! - lo!;
    };
    const ordered = (list: Segment[]) =>
      fanOut && !fanIn
        ? [...list].reverse()
        : fanIn && !fanOut
          ? list
          : [...list].sort((a, b) => length(b) - length(a));
    // A name its siblings carry too reads once, where their lines are one.
    const twin = flows.some(
      (flow) =>
        flow.index !== index &&
        flow.label === label &&
        (near(start(flow.index), start(index)) || near(end(flow.index), end(index))),
    );
    return twin ? [...ordered(shared), ...ordered(free)] : [...ordered(free), ...ordered(shared)];
  }

  function spots(segment: Segment, width: number, height: number): Box[] {
    const [lo, hi] = span(segment);
    const extent = segment.vertical ? height : width;
    if (hi! - lo! < extent + AIR.end * 2) return [];
    return ALONG.map((t) => {
      const along = Math.min(
        hi! - AIR.end - extent / 2,
        Math.max(lo! + AIR.end + extent / 2, lo! + (hi! - lo!) * t),
      );
      const centre = segment.vertical ? { x: segment.a.x, y: along } : { x: along, y: segment.a.y };
      return { x: centre.x - width / 2, y: centre.y - height / 2, width, height };
    });
  }

  const clear = (box: Box, index: number, segment: Segment) =>
    parts.every((part) => !meets(box, part, AIR.part)) &&
    placed.every((other) => !meets(box, other, AIR.plate)) &&
    all.every(
      (other) =>
        other.flow === index ||
        overlaps(segment, other) ||
        !crosses(other, {
          x: box.x - 5,
          y: box.y - 5,
          width: box.width + 10,
          height: box.height + 10,
        }),
    );

  // The work's own path first, then side channels and replies; shorter names before longer.
  const order = flows
    .filter((flow) => flow.label)
    .sort(
      (a, b) =>
        Number(layout.flows[a.index]!.quiet) - Number(layout.flows[b.index]!.quiet) ||
        a.label.length - b.label.length ||
        a.index - b.index,
    );
  for (const flow of order) {
    if (out.has(flow.index)) continue;
    const width = plateWidth(flow.label);
    const height = plateHeight(flow.label);
    const runs = candidates(flow.index, flow.label);
    let found: { box: Box; segment: Segment } | null = null;
    for (const segment of runs) {
      for (const box of spots(segment, width, height))
        if (clear(box, flow.index, segment)) {
          found = { box, segment };
          break;
        }
      if (found) break;
    }
    if (found) {
      placed.push(found.box);
      const at = { x: found.box.x + width / 2, y: found.box.y + height / 2 };
      out.set(flow.index, { at, resting: true });
      // Flows that share this run and this name read it here too.
      for (const other of sharers(found.segment))
        if (!out.has(other.flow)) out.set(other.flow, { at, resting: true });
      continue;
    }
    // Shown only when its part is looked at: on its own longest free run, or its middle.
    const fallback = runs.flatMap((segment) => spots(segment, width, height))[0];
    const points = layout.flows[flow.index]!.points;
    const middle = points[Math.floor(points.length / 2)]!;
    const before = points[Math.floor(points.length / 2) - 1] ?? middle;
    out.set(flow.index, {
      at: fallback
        ? { x: fallback.x + width / 2, y: fallback.y + height / 2 }
        : { x: (middle.x + before.x) / 2, y: (middle.y + before.y) / 2 },
      resting: false,
    });
  }
  return out;
}

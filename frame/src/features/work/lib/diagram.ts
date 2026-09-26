import type { ArtifactContent } from "$shared/ui/data/Artifact/artifact";
import type { CanvasPosition, CanvasSize } from "./canvas-model";
import * as m from "$shared/i18n/messages";

type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;

/** A part's card, the air between columns and rows, and the band a layer's caption takes. */
export const DIAGRAM = {
  node: { width: 220, height: 72 },
  column: 32,
  row: 12,
  layer: 16,
} as const satisfies Record<string, number | CanvasSize>;

/**
 * A flow's plate: caption characters at 6.5 px and 12 px of padding, up to
 * two 15 px lines once the words pass the widest plate; `air` keeps it clear
 * of the parts and of the plate beside it.
 */
export const PLATE = { char: 6.5, pad: 12, line: 15, height: 17, air: 6, max: 220 } as const;
const plateRun = (label: string) => Math.ceil(label.trim().length * PLATE.char + PLATE.pad);
export const plateWidth = (label: string) => Math.min(PLATE.max, plateRun(label));
export const plateHeight = (label: string) =>
  plateRun(label) > PLATE.max ? PLATE.height + PLATE.line : PLATE.height;

/** A level cubic's control points: it leaves and lands along its handles. */
function levelCurve(sx: number, sy: number, tx: number, ty: number) {
  const reach = Math.max(24, Math.abs(tx - sx) / 2);
  return [sx + reach, sy, tx - reach, ty] as const;
}
const cubic = (a: number, b: number, c: number, d: number, t: number) =>
  (1 - t) ** 3 * a + 3 * (1 - t) ** 2 * t * b + 3 * (1 - t) * t ** 2 * c + t ** 3 * d;
/** The height at which a level curve crosses `x`. */
export function curveY(sx: number, sy: number, tx: number, ty: number, x: number): number {
  const [c1x, , c2x] = levelCurve(sx, sy, tx, ty);
  let low = 0;
  let high = 1;
  for (let step = 0; step < 32; step += 1) {
    const t = (low + high) / 2;
    if (cubic(sx, c1x, c2x, tx, t) < x) low = t;
    else high = t;
  }
  return cubic(sy, sy, ty, ty, (low + high) / 2);
}

type Side = "top" | "right" | "bottom" | "left";
const OUT: Record<Side, readonly [number, number]> = {
  top: [0, -1],
  right: [1, 0],
  bottom: [0, 1],
  left: [-1, 0],
};
export type FlowCurve = readonly [number, number, number, number, number, number, number, number];
/**
 * A cubic that leaves and lands along its handles: level across columns,
 * upright down a column, and bent out by `reach` when both ends face right.
 */
export function flowCurve(
  sx: number,
  sy: number,
  from: Side,
  tx: number,
  ty: number,
  to: Side,
  reach?: number,
): FlowCurve {
  const upright = from === "top" || from === "bottom";
  const along = reach ?? Math.max(24, (upright ? Math.abs(ty - sy) : Math.abs(tx - sx)) / 2);
  const [ax, ay] = OUT[from];
  const [bx, by] = OUT[to];
  return [sx, sy, sx + ax * along, sy + ay * along, tx + bx * along, ty + by * along, tx, ty];
}
/** The point `t` of the way along a flow's curve. */
export const pointOn = ([x0, y0, x1, y1, x2, y2, x3, y3]: FlowCurve, t: number) => ({
  x: cubic(x0, x1, x2, x3, t),
  y: cubic(y0, y1, y2, y3, t),
});

/**
 * How a flow between two parts of one column runs: straight down or up the
 * connector when the parts are neighbours, otherwise bent out into the gap
 * beside the column by `beside` from both parts' right edges.
 */
export type DiagramRoute = "down" | "up" | { beside: number };
/**
 * Where a flow's plate stands. A flow to a column further right sits in the
 * gap before its target, on its curve, moved down by `shift` when it would
 * overlap another; a flow down one column sits `along` its own line; a flow
 * back left sits at an offset from its target's handle.
 */
export type DiagramPlate =
  { gap: number; shift: number } | { along: number } | { dx: number; dy: number };

/** A part's card: its diagram's result card and the part's own id. */
export const diagramNodeId = (result: string, node: string) => `diagram:${result}:${node}`;
/** The result card a part belongs to. */
export const diagramResult = (id: string) =>
  id.startsWith("diagram:") ? id.slice(8, id.lastIndexOf(":")) : null;

/**
 * The parts in columns, left to right. Stated layers are the columns, in the
 * order the diagram lists them; otherwise a part stands one column right of
 * the furthest part that flows into it, sources first. Within a column parts
 * stack in the order the edges first name them.
 */
export function diagramColumns(diagram: Diagram): { layer?: string; nodes: string[] }[] {
  const ids = diagram.nodes.map((node) => node.id);
  const known = new Set(ids);
  const edges = diagram.edges.filter(
    (edge) => edge.from !== edge.to && known.has(edge.from) && known.has(edge.to),
  );
  const mention = new Map<string, number>();
  edges.forEach((edge, index) => {
    if (!mention.has(edge.from)) mention.set(edge.from, index * 2);
    if (!mention.has(edge.to)) mention.set(edge.to, index * 2 + 1);
  });
  const order = (a: string, b: string) =>
    (mention.get(a) ?? Infinity) - (mention.get(b) ?? Infinity) || ids.indexOf(a) - ids.indexOf(b);
  const layers = diagram.layers.filter((layer) =>
    diagram.nodes.some((node) => node.layer === layer.id),
  );
  if (layers.length) {
    const columns: { layer?: string; nodes: string[] }[] = layers.map((layer) => ({
      layer: layer.name,
      nodes: diagram.nodes.filter((node) => node.layer === layer.id).map((node) => node.id),
    }));
    const loose = diagram.nodes.filter((node) => !layers.some((layer) => layer.id === node.layer));
    if (loose.length) columns.push({ nodes: loose.map((node) => node.id) });
    return columns.map((column) => ({ ...column, nodes: [...column.nodes].sort(order) }));
  }
  // A cycle has no longest path: the edge that closes it is not followed.
  const outgoing = new Map(ids.map((id) => [id, [] as string[]]));
  const incoming = new Map(ids.map((id) => [id, 0]));
  for (const edge of edges) {
    outgoing.get(edge.from)!.push(edge.to);
    incoming.set(edge.to, incoming.get(edge.to)! + 1);
  }
  const forward = new Map(ids.map((id) => [id, [] as string[]]));
  const seen = new Map<string, "open" | "done">();
  const visit = (id: string) => {
    seen.set(id, "open");
    for (const next of outgoing.get(id)!) {
      if (seen.get(next) === "open") continue;
      forward.get(id)!.push(next);
      if (!seen.has(next)) visit(next);
    }
    seen.set(id, "done");
  };
  for (const id of [...ids.filter((id) => !incoming.get(id)), ...ids]) if (!seen.has(id)) visit(id);
  const rank = new Map(ids.map((id) => [id, 0]));
  const waiting = new Map(ids.map((id) => [id, 0]));
  for (const targets of forward.values())
    for (const target of targets) waiting.set(target, waiting.get(target)! + 1);
  const ready = ids.filter((id) => !waiting.get(id));
  while (ready.length) {
    const id = ready.shift()!;
    for (const next of forward.get(id)!) {
      rank.set(next, Math.max(rank.get(next)!, rank.get(id)! + 1));
      waiting.set(next, waiting.get(next)! - 1);
      if (!waiting.get(next)) ready.push(next);
    }
  }
  const columns: string[][] = [];
  for (const id of ids) (columns[rank.get(id)!] ??= []).push(id);
  return columns.filter(Boolean).map((nodes) => ({ nodes: nodes.sort(order) }));
}

/**
 * Where each part stands inside the diagram's area, from the area's first
 * card: rows 12 px apart, or apart by a plate and its air where a named flow
 * joins neighbours in one column; a band above for the layers' captions when
 * there are layers; and columns 32 px apart plus the widest plate that sits
 * between them, so every flow's name has room of its own on its own line.
 */
export function diagramLayout(diagram: Diagram): {
  at: Record<string, CanvasPosition>;
  width: number;
  height: number;
  layers: { name: string; nodes: string[] }[];
  plates: Record<number, DiagramPlate>;
  routes: Record<number, DiagramRoute>;
} {
  const columns = diagramColumns(diagram);
  if (!columns.length) return { at: {}, width: 0, height: 0, layers: [], plates: {}, routes: {} };
  const band = columns.some((column) => column.layer) ? DIAGRAM.layer : 0;
  const { width, height } = DIAGRAM.node;
  const place = new Map<string, { column: number; row: number }>();
  columns.forEach((column, index) =>
    column.nodes.forEach((id, row) => place.set(id, { column: index, row })),
  );
  const flows = diagram.edges.flatMap((edge, index) => {
    const from = place.get(edge.from);
    const to = place.get(edge.to);
    if (!from || !to || edge.from === edge.to) return [];
    const label = edge.label?.trim() ?? "";
    const kind: "forward" | "back" | "down" | "beside" =
      to.column > from.column
        ? "forward"
        : to.column < from.column
          ? "back"
          : Math.abs(to.row - from.row) === 1
            ? "down"
            : "beside";
    // A flow beside its column takes the gap to the column's right; any other the gap before its target.
    const gap = kind === "beside" ? from.column : Math.max(0, to.column - 1);
    return [
      {
        index,
        from,
        to,
        kind,
        gap,
        label,
        width: label ? plateWidth(label) : 0,
        tall: label ? plateHeight(label) : 0,
      },
    ];
  });
  const gaps = columns.map((_, index) => {
    const own = flows.filter((flow) => flow.gap === index && flow.kind !== "down");
    const widest = Math.max(0, ...own.filter((flow) => flow.label).map((flow) => flow.width));
    const used = widest > 0 || own.some((flow) => flow.kind === "beside");
    return index < columns.length - 1 || used ? DIAGRAM.column + widest : 0;
  });
  // A named flow between neighbours in one column opens their row gap to hold its plate.
  const rows = Math.max(...columns.map((column) => column.nodes.length));
  const between = Array.from({ length: Math.max(0, rows - 1) }, (_, row) =>
    Math.max(
      DIAGRAM.row,
      ...flows
        .filter((flow) => flow.kind === "down" && flow.label)
        .filter((flow) => Math.min(flow.from.row, flow.to.row) === row)
        .map((flow) => flow.tall + PLATE.air * 2),
    ),
  );
  const left: number[] = [0];
  for (let index = 1; index < columns.length; index += 1)
    left.push(left[index - 1]! + width + gaps[index - 1]!);
  const top = (row: number) =>
    band + row * height + between.slice(0, row).reduce((sum, gap) => sum + gap, 0);
  const at: Record<string, CanvasPosition> = {};
  for (const [id, spot] of place) at[id] = { x: left[spot.column]!, y: top(spot.row) };
  let bottom = top(rows - 1) + height;
  const centre = (gap: number) => left[gap]! + width + gaps[gap]! / 2;
  const routes: Record<number, DiagramRoute> = {};
  const plates: Record<number, DiagramPlate> = {};
  const settled = flows.flatMap((flow) => {
    const source = { x: left[flow.from.column]!, y: top(flow.from.row) };
    const target = { x: left[flow.to.column]!, y: top(flow.to.row) };
    if (flow.kind === "down") {
      routes[flow.index] = flow.to.row > flow.from.row ? "down" : "up";
      if (flow.label) plates[flow.index] = { along: 0.5 };
      return [];
    }
    // Bent out so the curve's far point is the middle of the gap: 3/4 of its reach.
    const reach = Math.round((gaps[flow.gap]! / 2) * (4 / 3));
    if (flow.kind === "beside") routes[flow.index] = { beside: reach };
    if (!flow.label) return [];
    const natural =
      flow.kind === "forward"
        ? curveY(
            source.x + width,
            source.y + height / 2,
            target.x,
            target.y + height / 2,
            centre(flow.gap),
          )
        : flow.kind === "beside"
          ? (source.y + target.y + height) / 2
          : target.y + height / 2;
    return [{ ...flow, source, target, reach, natural, y: natural }];
  });
  // Plates sharing a gap stagger by their heights instead of overlapping.
  for (const gap of new Set(settled.map((plate) => plate.gap))) {
    let last: { y: number; tall: number } | null = null;
    for (const plate of settled
      .filter((entry) => entry.gap === gap)
      .sort((a, b) => a.natural - b.natural || a.index - b.index)) {
      if (last)
        plate.y = Math.max(plate.natural, last.y + (last.tall + plate.tall) / 2 + PLATE.air);
      last = plate;
      bottom = Math.max(bottom, Math.ceil(plate.y + plate.tall / 2));
    }
  }
  for (const plate of settled) {
    const x = centre(plate.gap);
    plates[plate.index] =
      plate.kind === "forward"
        ? { gap: gaps[plate.gap]!, shift: Math.round(plate.y - plate.natural) }
        : plate.kind === "beside"
          ? { along: besideAt(plate.source.y, plate.target.y, plate.y, height) }
          : {
              dx: Math.round(x - plate.target.x),
              dy: Math.round(plate.y - plate.target.y - height / 2),
            };
  }
  return {
    at,
    width: left[columns.length - 1]! + width + gaps[columns.length - 1]!,
    height: bottom,
    layers: columns.flatMap((column) =>
      column.layer ? [{ name: column.layer, nodes: column.nodes }] : [],
    ),
    plates,
    routes,
  };
}

/** How far along a flow beside its column its plate stands to sit at height `y`. */
function besideAt(source: number, target: number, y: number, height: number): number {
  const span = target - source;
  const share = span ? Math.min(0.9, Math.max(0.1, (y - source - height / 2) / span)) : 0.5;
  // The curve's height runs 3t² − 2t³ of the way from one handle to the other.
  let low = 0;
  let high = 1;
  for (let step = 0; step < 24; step += 1) {
    const t = (low + high) / 2;
    if (3 * t ** 2 - 2 * t ** 3 < share) low = t;
    else high = t;
  }
  return Math.round(((low + high) / 2) * 1000) / 1000;
}

const KINDS: Record<string, () => string> = {
  client: m.work_diagram_client,
  edge: m.work_diagram_edge,
  gateway: m.work_diagram_gateway,
  service: m.work_diagram_service,
  worker: m.work_diagram_worker,
  model: m.work_diagram_model,
  store: m.work_diagram_store,
  queue: m.work_diagram_queue,
  cache: m.work_diagram_cache,
  storage: m.work_diagram_storage,
  external: m.work_diagram_external,
};
/** What a part is, in a word; an unnamed kind says nothing. */
export const diagramKindLabel = (kind: string) => KINDS[kind]?.() ?? m.work_diagram_other();

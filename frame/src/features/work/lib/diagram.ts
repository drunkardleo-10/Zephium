import { SvelteMap } from "svelte/reactivity";
import type { ArtifactContent } from "$shared/ui/data/Artifact/artifact";
import type { CanvasPosition, CanvasSize } from "./canvas-model";
import * as m from "$shared/i18n/messages";

type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;
type Rect = CanvasPosition & CanvasSize;

/** A part's card, the air between columns and rows, and the band a layer's caption takes. */
export const DIAGRAM = {
  node: { width: 220, height: 72 },
  column: 32,
  row: 12,
  layer: 16,
} as const satisfies Record<string, number | CanvasSize>;

/**
 * A flow's plate: caption characters at 6.5 px and 12 px of padding, up to
 * two 15 px lines once the words pass the widest plate.
 */
export const PLATE = { char: 6.5, pad: 12, line: 15, height: 17, air: 6, max: 220 } as const;
const plateRun = (label: string) => Math.ceil(label.trim().length * PLATE.char + PLATE.pad);
export const plateWidth = (label: string) => Math.min(PLATE.max, plateRun(label));
export const plateHeight = (label: string) =>
  plateRun(label) > PLATE.max ? PLATE.height + PLATE.line : PLATE.height;

/** Retired with curved flows: a flow now carries its own route. */
export type DiagramRoute = never;
export type DiagramPlate = never;

/** A part's card: its diagram's result card and the part's own id. */
export const diagramNodeId = (result: string, node: string) => `diagram:${result}:${node}`;
/** The result card a part belongs to. */
export const diagramResult = (id: string) =>
  id.startsWith("diagram:") ? id.slice(8, id.lastIndexOf(":")) : null;

/**
 * What a layout depends on and nothing else: the parts with their lane (the
 * stated layer's place, `-1` without layers), the named lanes, and the flows
 * between known parts by their index in the diagram.
 */
export type DiagramShape = {
  lanes: string[];
  parts: { id: string; lane: number }[];
  flows: { index: number; from: string; to: string; label: string }[];
};

export function diagramShape(diagram: Diagram): DiagramShape {
  const used = diagram.layers.filter((layer) =>
    diagram.nodes.some((node) => node.layer === layer.id),
  );
  const lane = (layer: string | undefined) => {
    if (!used.length) return -1;
    const at = used.findIndex((entry) => entry.id === layer);
    return at < 0 ? used.length : at;
  };
  return shapeOf(
    used.map((layer) => layer.name),
    diagram.nodes.map((node) => ({ id: node.id, lane: lane(node.layer) })),
    diagram.edges.map((edge, index) => ({ index, ...edge })),
  );
}

/**
 * The shape of a diagram as the canvas holds it: its area's members, its
 * layers' members, and its flows' links, so the canvas finds the layout the
 * lane was placed by.
 */
export function sceneShape(
  result: string,
  members: readonly string[],
  layers: readonly { name: string; members: readonly string[] }[],
  links: readonly { id: string; source: string; target: string; label?: string }[],
): DiagramShape {
  const prefix = diagramNodeId(result, "");
  const local = (id: string) => (id.startsWith(prefix) ? id.slice(prefix.length) : id);
  const lane = (id: string) => {
    if (!layers.length) return -1;
    const at = layers.findIndex((layer) => layer.members.includes(id));
    return at < 0 ? layers.length : at;
  };
  return shapeOf(
    layers.map((layer) => layer.name),
    members.map((id) => ({ id: local(id), lane: lane(id) })),
    links.map((link) => ({
      index: Number(link.id.slice(link.id.lastIndexOf(":") + 1)),
      from: local(link.source),
      to: local(link.target),
      ...(link.label === undefined ? {} : { label: link.label }),
    })),
  );
}

function shapeOf(
  lanes: string[],
  parts: DiagramShape["parts"],
  edges: { index: number; from: string; to: string; label?: string }[],
): DiagramShape {
  const known = new Set(parts.map((part) => part.id));
  return {
    lanes,
    parts,
    flows: edges
      .filter((edge) => edge.from !== edge.to && known.has(edge.from) && known.has(edge.to))
      .map((edge) => ({
        index: edge.index,
        from: edge.from,
        to: edge.to,
        label: edge.label?.trim() ?? "",
      })),
  };
}

/**
 * A flow drawn at rest: it is named, and it is the first flow out of its
 * source or the first into its target. The rest wait for their parts.
 */
export function primaryFlows(shape: DiagramShape): Set<number> {
  const out = new Set<string>();
  const into = new Set<string>();
  const primary = new Set<number>();
  for (const flow of shape.flows) {
    const first = !out.has(flow.from) || !into.has(flow.to);
    out.add(flow.from);
    into.add(flow.to);
    if (first && flow.label) primary.add(flow.index);
  }
  return primary;
}

/** One flow drawn: an orthogonal line from source to target, its plate's centre on it. */
export type DiagramFlow = {
  from: string;
  to: string;
  points: CanvasPosition[];
  plate?: CanvasPosition;
  primary: boolean;
};
/** A layer's swimlane: a band from the picture's top to its bottom. */
export type DiagramBand = Rect & { name: string };
/**
 * Where everything stands, from the first part's corner (under the layers'
 * caption band): parts, flows by their index, bands, and the whole picture's
 * `bounds`, which may reach above or left of the first part. `width` and
 * `height` are the picture's reach from that corner, at most 4096.
 */
export type DiagramLayout = {
  at: Record<string, CanvasPosition>;
  width: number;
  height: number;
  bounds: Rect;
  layers: { name: string; nodes: string[] }[];
  bands: DiagramBand[];
  flows: Record<number, DiagramFlow>;
  plates: Record<number, DiagramPlate>;
  routes: Record<number, DiagramRoute>;
  /** Laid out by the layout engine rather than in columns. */
  settled: boolean;
};
/** The largest element the canvas contract admits. */
export const DIAGRAM_REACH = 4096;

/**
 * The parts in columns, left to right. Stated layers are the columns, in the
 * order the diagram lists them; otherwise a part stands one column right of
 * the furthest part that flows into it, sources first. Within a column parts
 * stack in the order the edges first name them.
 */
export function diagramColumns(diagram: Diagram): { layer?: string; nodes: string[] }[] {
  const shape = diagramShape(diagram);
  return columnsOf(shape).map((nodes, index) => ({
    ...(shape.lanes[index] === undefined ? {} : { layer: shape.lanes[index] }),
    nodes,
  }));
}

function columnsOf(shape: DiagramShape): string[][] {
  const ids = shape.parts.map((part) => part.id);
  const mention = new Map<string, number>();
  shape.flows.forEach((edge, index) => {
    if (!mention.has(edge.from)) mention.set(edge.from, index * 2);
    if (!mention.has(edge.to)) mention.set(edge.to, index * 2 + 1);
  });
  const order = (a: string, b: string) =>
    (mention.get(a) ?? Infinity) - (mention.get(b) ?? Infinity) || ids.indexOf(a) - ids.indexOf(b);
  if (shape.lanes.length) {
    const columns: string[][] = [];
    for (const part of shape.parts) (columns[part.lane] ??= []).push(part.id);
    return columns.filter(Boolean).map((column) => column.sort(order));
  }
  // A cycle has no longest path: the edge that closes it is not followed.
  const outgoing = new Map(ids.map((id) => [id, [] as string[]]));
  const incoming = new Map(ids.map((id) => [id, 0]));
  for (const edge of shape.flows) {
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
  return columns.filter(Boolean).map((nodes) => nodes.sort(order));
}

/**
 * The layout while the engine answers, and wherever it cannot run: parts in
 * columns 32 px apart plus the widest plate between them, rows 12 px apart
 * or apart by a plate where a named flow joins neighbours in one column, and
 * each flow an elbow between its parts.
 */
export function columnLayout(shape: DiagramShape): DiagramLayout {
  const columns = columnsOf(shape);
  const empty = { x: 0, y: 0, width: 0, height: 0 };
  if (!columns.length)
    return {
      at: {},
      width: 0,
      height: 0,
      bounds: empty,
      layers: [],
      bands: [],
      flows: {},
      plates: {},
      routes: {},
      settled: false,
    };
  const band = shape.lanes.length ? DIAGRAM.layer : 0;
  const { width, height } = DIAGRAM.node;
  const place = new Map<string, { column: number; row: number }>();
  columns.forEach((column, index) =>
    column.forEach((id, row) => place.set(id, { column: index, row })),
  );
  const flows = shape.flows.map((flow) => {
    const from = place.get(flow.from)!;
    const to = place.get(flow.to)!;
    const kind: "across" | "down" | "beside" =
      to.column !== from.column ? "across" : Math.abs(to.row - from.row) === 1 ? "down" : "beside";
    const gap = kind === "beside" ? from.column : Math.max(0, Math.max(to.column, from.column) - 1);
    return { ...flow, source: flow.from, target: flow.to, from, to, kind, gap };
  });
  const gaps = columns.map((_, index) => {
    const own = flows.filter((flow) => flow.gap === index && flow.kind !== "down");
    const widest = Math.max(0, ...own.filter((flow) => flow.label).map((f) => plateWidth(f.label)));
    const used = widest > 0 || own.some((flow) => flow.kind === "beside");
    return index < columns.length - 1 || used ? DIAGRAM.column + widest : 0;
  });
  const rows = Math.max(...columns.map((column) => column.length));
  const between = Array.from({ length: Math.max(0, rows - 1) }, (_, row) =>
    Math.max(
      DIAGRAM.row,
      ...flows
        .filter((flow) => flow.kind === "down" && flow.label)
        .filter((flow) => Math.min(flow.from.row, flow.to.row) === row)
        .map((flow) => plateHeight(flow.label) + PLATE.air * 2),
    ),
  );
  const left: number[] = [0];
  for (let index = 1; index < columns.length; index += 1)
    left.push(left[index - 1]! + width + gaps[index - 1]!);
  const top = (row: number) =>
    band + row * height + between.slice(0, row).reduce((sum, gap) => sum + gap, 0);
  const at: Record<string, CanvasPosition> = {};
  for (const [id, spot] of place) at[id] = { x: left[spot.column]!, y: top(spot.row) };
  const primary = primaryFlows(shape);
  const drawn: Record<number, DiagramFlow> = {};
  for (const flow of flows) {
    const a = { ...at[flow.source]!, width, height };
    const b = { ...at[flow.target]!, width, height };
    const points =
      flow.kind === "beside"
        ? [
            { x: a.x + width, y: a.y + height / 2 },
            { x: a.x + width + gaps[flow.gap]! / 2, y: a.y + height / 2 },
            { x: b.x + width + gaps[flow.gap]! / 2, y: b.y + height / 2 },
            { x: b.x + width, y: b.y + height / 2 },
          ]
        : elbow(a, b);
    drawn[flow.index] = {
      from: flow.source,
      to: flow.target,
      points,
      ...(flow.label ? { plate: midpoint(points) } : {}),
      primary: primary.has(flow.index),
    };
  }
  const reach = left[columns.length - 1]! + width + gaps[columns.length - 1]!;
  const bottom = top(rows - 1) + height;
  return {
    at,
    width: Math.min(DIAGRAM_REACH, reach),
    height: Math.min(DIAGRAM_REACH, bottom),
    bounds: { x: 0, y: 0, width: reach, height: bottom },
    layers: lanesOf(shape),
    bands: bandsOf(shape, at, { x: 0, y: 0, width: reach, height: bottom }, 0),
    flows: drawn,
    plates: {},
    routes: {},
    settled: false,
  };
}

/** Each named lane's parts, in order. */
export const lanesOf = (shape: DiagramShape) =>
  shape.lanes.flatMap((name, lane) => {
    const nodes = shape.parts.filter((part) => part.lane === lane).map((part) => part.id);
    return nodes.length ? [{ name, nodes }] : [];
  });

/** A named lane's band: around its parts by `pad`, the picture's whole height. */
export function bandsOf(
  shape: DiagramShape,
  at: Record<string, CanvasPosition>,
  bounds: Rect,
  pad: number,
): DiagramBand[] {
  return lanesOf(shape).map(({ name, nodes }) => {
    const xs = nodes.map((id) => at[id]!.x);
    const x = Math.min(...xs) - pad;
    return {
      name,
      x,
      y: bounds.y,
      width: Math.max(...xs) + DIAGRAM.node.width + pad - x,
      height: bounds.height,
    };
  });
}

/**
 * An orthogonal line between two boxes: across the gap between them with one
 * step at its middle, or down or up when they share columns; beside both
 * when they overlap.
 */
export function elbow(a: Rect, b: Rect): CanvasPosition[] {
  const ay = a.y + a.height / 2;
  const by = b.y + b.height / 2;
  const ax = a.x + a.width / 2;
  const bx = b.x + b.width / 2;
  const step = (from: CanvasPosition, to: CanvasPosition, across: boolean) => {
    if (across ? from.y === to.y : from.x === to.x) return [from, to];
    if (across) {
      const x = (from.x + to.x) / 2;
      return [from, { x, y: from.y }, { x, y: to.y }, to];
    }
    const y = (from.y + to.y) / 2;
    return [from, { x: from.x, y }, { x: to.x, y }, to];
  };
  if (b.x >= a.x + a.width) return step({ x: a.x + a.width, y: ay }, { x: b.x, y: by }, true);
  if (b.x + b.width <= a.x) return step({ x: a.x, y: ay }, { x: b.x + b.width, y: by }, true);
  if (b.y >= a.y + a.height) return step({ x: ax, y: a.y + a.height }, { x: bx, y: b.y }, false);
  if (b.y + b.height <= a.y) return step({ x: ax, y: a.y }, { x: bx, y: b.y + b.height }, false);
  const x = Math.max(a.x + a.width, b.x + b.width) + 24;
  return [
    { x: a.x + a.width, y: ay },
    { x, y: ay },
    { x, y: by },
    { x: b.x + b.width, y: by },
  ];
}

/** The point halfway along a line. */
export function midpoint(points: readonly CanvasPosition[]): CanvasPosition {
  const lengths = points.slice(1).map((p, i) => Math.hypot(p.x - points[i]!.x, p.y - points[i]!.y));
  let rest = lengths.reduce((sum, length) => sum + length, 0) / 2;
  for (let i = 0; i < lengths.length; i += 1) {
    const a = points[i]!;
    const b = points[i + 1]!;
    if (rest <= lengths[i]! && lengths[i]) {
      const t = rest / lengths[i]!;
      return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
    }
    rest -= lengths[i]!;
  }
  return points[0] ?? { x: 0, y: 0 };
}

/** A flow's line: 8 px rounded corners, stopping `short` before its target for the arrowhead. */
export function flowPath(points: readonly CanvasPosition[], short = 0, radius = 8): string {
  if (points.length < 2) return "";
  const end = points.at(-1)!;
  const last = points.at(-2)!;
  const run = Math.hypot(end.x - last.x, end.y - last.y) || 1;
  const cut = Math.min(short, run);
  const stop = {
    x: end.x - ((end.x - last.x) / run) * cut,
    y: end.y - ((end.y - last.y) / run) * cut,
  };
  const line = [...points.slice(0, -1), stop];
  let d = `M ${line[0]!.x},${line[0]!.y}`;
  for (let i = 1; i < line.length - 1; i += 1) {
    const [p, c, n] = [line[i - 1]!, line[i]!, line[i + 1]!];
    const inLength = Math.hypot(c.x - p.x, c.y - p.y);
    const outLength = Math.hypot(n.x - c.x, n.y - c.y);
    const r = Math.min(radius, inLength / 2, outLength / 2);
    if (!r) {
      d += ` L ${c.x},${c.y}`;
      continue;
    }
    const a = { x: c.x - ((c.x - p.x) / inLength) * r, y: c.y - ((c.y - p.y) / inLength) * r };
    const b = { x: c.x + ((n.x - c.x) / outLength) * r, y: c.y + ((n.y - c.y) / outLength) * r };
    d += ` L ${a.x},${a.y} Q ${c.x},${c.y} ${b.x},${b.y}`;
  }
  return `${d} L ${stop.x},${stop.y}`;
}

/** The arrowhead: `size` long, landing on the line's last point along its last segment. */
export function arrowHead(points: readonly CanvasPosition[], size = 5): string {
  const end = points.at(-1);
  const last = points.at(-2);
  if (!end || !last) return "";
  const run = Math.hypot(end.x - last.x, end.y - last.y) || 1;
  const [dx, dy] = [(end.x - last.x) / run, (end.y - last.y) / run];
  const base = { x: end.x - dx * size, y: end.y - dy * size };
  const half = size * 0.7;
  return `M ${end.x},${end.y} L ${base.x - dy * half},${base.y + dx * half} L ${base.x + dy * half},${base.y - dx * half} Z`;
}

/**
 * Where a diagram's picture stands on the canvas: the corner most of its parts
 * agree on, and how many do, so a part the person dragged away moves nothing else.
 */
export function diagramOrigin(
  spots: readonly { position: CanvasPosition; at: CanvasPosition }[],
): { origin: CanvasPosition; count: number } | null {
  const votes = new Map<string, { origin: CanvasPosition; count: number }>();
  let best: { origin: CanvasPosition; count: number } | null = null;
  for (const { position, at } of spots) {
    const origin = { x: Math.round(position.x - at.x), y: Math.round(position.y - at.y) };
    const key = `${origin.x},${origin.y}`;
    const vote = votes.get(key) ?? { origin, count: 0 };
    vote.count += 1;
    votes.set(key, vote);
    if (!best || vote.count > best.count) best = vote;
  }
  return best;
}

const KEYS: Record<string, CanvasPosition> = {
  ArrowRight: { x: 1, y: 0 },
  ArrowLeft: { x: -1, y: 0 },
  ArrowDown: { x: 0, y: 1 },
  ArrowUp: { x: 0, y: -1 },
};
/**
 * The part an arrow key moves to from `from`: of the parts a flow joins it to,
 * the nearest that lies that way, a sideways step costing twice a straight one.
 */
export function partAlong(
  from: CanvasPosition,
  joined: readonly { id: string; at: CanvasPosition }[],
  key: string,
): string | null {
  const way = KEYS[key];
  if (!way) return null;
  let best: { id: string; cost: number } | null = null;
  for (const { id, at } of joined) {
    const dx = at.x - from.x;
    const dy = at.y - from.y;
    const along = dx * way.x + dy * way.y;
    if (along <= 0) continue;
    const cost = along + 2 * Math.abs(dx * way.y - dy * way.x);
    if (!best || cost < best.cost) best = { id, cost };
  }
  return best?.id ?? null;
}

// Layouts the engine settled, by shape; reading one makes a derivation wait for it.
const settled = new SvelteMap<string, DiagramLayout>();
const columned = new Map<string, DiagramLayout>();
const asked = new Set<string>();
const KEPT = 48;
const keyOf = (shape: DiagramShape) => JSON.stringify(shape);

/** The layout the engine settled for a shape, if it has. */
export const settledLayout = (shape: DiagramShape) => settled.get(keyOf(shape));

/**
 * A diagram's layout now: the engine's once it has answered, otherwise the
 * columns while it is asked (in a browser only), so an area appears at once
 * and settles when the answer lands.
 */
export function diagramLayout(diagram: Diagram): DiagramLayout {
  return layoutOf(diagramShape(diagram));
}

export function layoutOf(shape: DiagramShape): DiagramLayout {
  const key = keyOf(shape);
  const done = settled.get(key);
  if (done) return done;
  if (typeof window !== "undefined" && !asked.has(key)) void settle(shape, key);
  let columns = columned.get(key);
  if (!columns) {
    columns = columnLayout(shape);
    if (columned.size >= KEPT) columned.delete(columned.keys().next().value!);
    columned.set(key, columns);
  }
  return columns;
}

/** A diagram laid out by the engine, off the main thread where the page allows it. */
export async function layoutDiagram(diagram: Diagram): Promise<DiagramLayout> {
  const shape = diagramShape(diagram);
  return settled.get(keyOf(shape)) ?? (await settle(shape, keyOf(shape)));
}

async function settle(shape: DiagramShape, key: string): Promise<DiagramLayout> {
  asked.add(key);
  try {
    const { engineLayout } = await import("./diagram-worker");
    const layout = await engineLayout(shape);
    if (settled.size >= KEPT) settled.delete(settled.keys().next().value!);
    settled.set(key, layout);
    return layout;
  } catch {
    // The columns stand for good; asking again would fail the same way.
    return columnLayout(shape);
  }
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

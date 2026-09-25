import type { ArtifactContent } from "$shared/ui/data/Artifact/artifact";
import type { CanvasPosition, CanvasSize } from "./canvas-model";
import * as m from "$shared/i18n/messages";

type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;

/** A part's card, the air between columns and rows, and the band a layer's caption takes. */
export const DIAGRAM = {
  node: { width: 200, height: 64 },
  column: 32,
  row: 12,
  layer: 16,
} as const satisfies Record<string, number | CanvasSize>;

/** A flow's plate: caption characters at 6.5 px, 12 px of padding, one line, capped. */
export const PLATE = { char: 6.5, pad: 12, height: 17, air: 2, max: 220 } as const;
export const plateWidth = (label: string) =>
  Math.min(PLATE.max, Math.ceil(label.trim().length * PLATE.char + PLATE.pad));

/** A level cubic's control points: it leaves and lands along its handles. */
export function levelCurve(sx: number, sy: number, tx: number, ty: number) {
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

/**
 * Where a flow's plate stands. A flow to a column further right sits in the
 * gap before its target, on its curve, moved down by `shift` when it would
 * overlap another; any other flow sits at an offset from its target's handle.
 */
export type DiagramPlate = { gap: number; shift: number } | { dx: number; dy: number };

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
 * card: rows 12 px apart, a band above for the layers' captions when there
 * are layers, and columns 32 px apart plus the widest plate that sits
 * between them, so every flow's name has room of its own.
 */
export function diagramLayout(diagram: Diagram): {
  at: Record<string, CanvasPosition>;
  width: number;
  height: number;
  layers: { name: string; nodes: string[] }[];
  plates: Record<number, DiagramPlate>;
} {
  const columns = diagramColumns(diagram);
  if (!columns.length) return { at: {}, width: 0, height: 0, layers: [], plates: {} };
  const band = columns.some((column) => column.layer) ? DIAGRAM.layer : 0;
  const { width, height } = DIAGRAM.node;
  const place = new Map<string, { column: number; row: number }>();
  columns.forEach((column, index) =>
    column.nodes.forEach((id, row) => place.set(id, { column: index, row })),
  );
  // Each named flow takes a gap: before its target, or beside a column it runs down.
  const named = diagram.edges.flatMap((edge, index) => {
    const from = place.get(edge.from);
    const to = place.get(edge.to);
    const label = edge.label?.trim() ?? "";
    if (!from || !to || edge.from === edge.to || !label) return [];
    const kind: "forward" | "down" | "back" =
      to.column > from.column ? "forward" : to.column === from.column ? "down" : "back";
    const gap = kind === "down" ? from.column : Math.max(0, to.column - 1);
    return [{ index, from, to, kind, gap, width: plateWidth(label) }];
  });
  const gaps = columns.map((_, index) =>
    Math.max(0, ...named.filter((plate) => plate.gap === index).map((plate) => plate.width)),
  );
  const air = gaps.map((widest, index) =>
    index < columns.length - 1 ? DIAGRAM.column + widest : widest ? DIAGRAM.column + widest : 0,
  );
  const left: number[] = [0];
  for (let index = 1; index < columns.length; index += 1)
    left.push(left[index - 1]! + width + air[index - 1]!);
  const top = (row: number) => band + row * (height + DIAGRAM.row);
  const at: Record<string, CanvasPosition> = {};
  let tallest = 0;
  for (const [id, spot] of place) {
    at[id] = { x: left[spot.column]!, y: top(spot.row) };
    tallest = Math.max(tallest, spot.row + 1);
  }
  let bottom = band + tallest * (height + DIAGRAM.row) - DIAGRAM.row;
  const centre = (gap: number) => left[gap]! + width + air[gap]! / 2;
  const settled = named.map((plate) => {
    const source = { x: left[plate.from.column]!, y: top(plate.from.row) };
    const target = { x: left[plate.to.column]!, y: top(plate.to.row) };
    const y =
      plate.kind === "forward"
        ? curveY(
            source.x + width,
            source.y + height / 2,
            target.x,
            target.y + height / 2,
            centre(plate.gap),
          )
        : plate.kind === "down"
          ? (source.y + target.y + height) / 2
          : target.y + height / 2;
    return { ...plate, source, target, natural: y, y };
  });
  // Plates sharing a gap stagger by one plate's height instead of overlapping.
  for (const gap of new Set(settled.map((plate) => plate.gap))) {
    let last = -Infinity;
    for (const plate of settled
      .filter((entry) => entry.gap === gap)
      .sort((a, b) => a.natural - b.natural || a.index - b.index)) {
      plate.y = Math.max(plate.natural, last + PLATE.height + PLATE.air);
      last = plate.y;
      bottom = Math.max(bottom, Math.ceil(plate.y + PLATE.height / 2));
    }
  }
  const plates: Record<number, DiagramPlate> = {};
  for (const plate of settled) {
    const x = centre(plate.gap);
    plates[plate.index] =
      plate.kind === "forward"
        ? { gap: air[plate.gap]!, shift: Math.round(plate.y - plate.natural) }
        : plate.kind === "down"
          ? {
              dx: Math.round(x - plate.target.x - width / 2),
              dy: Math.round(plate.y - plate.target.y),
            }
          : {
              dx: Math.round(x - plate.target.x),
              dy: Math.round(plate.y - plate.target.y - height / 2),
            };
  }
  return {
    at,
    width: left[columns.length - 1]! + width + air[columns.length - 1]!,
    height: bottom,
    layers: columns.flatMap((column) =>
      column.layer ? [{ name: column.layer, nodes: column.nodes }] : [],
    ),
    plates,
  };
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

import type { ArtifactContent } from "$shared/ui/data/Artifact/artifact";
import type { CanvasPosition, CanvasSize } from "./canvas-model";
import * as m from "$shared/i18n/messages";

type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;

/** A part's card, the air between columns and rows, and the band a layer's caption takes. */
export const DIAGRAM = {
  node: { width: 180, height: 56 },
  column: 32,
  row: 12,
  layer: 16,
} as const satisfies Record<string, number | CanvasSize>;

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
 * card: columns 32 px apart, rows 12 px apart, a band above for the layers'
 * captions when there are layers.
 */
export function diagramLayout(diagram: Diagram): {
  at: Record<string, CanvasPosition>;
  width: number;
  height: number;
  layers: { name: string; nodes: string[] }[];
} {
  const columns = diagramColumns(diagram);
  const band = columns.some((column) => column.layer) ? DIAGRAM.layer : 0;
  const { width, height } = DIAGRAM.node;
  const at: Record<string, CanvasPosition> = {};
  let tallest = 0;
  columns.forEach((column, index) =>
    column.nodes.forEach((id, row) => {
      at[id] = { x: index * (width + DIAGRAM.column), y: band + row * (height + DIAGRAM.row) };
      tallest = Math.max(tallest, row + 1);
    }),
  );
  return {
    at,
    width: columns.length * (width + DIAGRAM.column) - DIAGRAM.column,
    height: band + tallest * (height + DIAGRAM.row) - DIAGRAM.row,
    layers: columns.flatMap((column) =>
      column.layer ? [{ name: column.layer, nodes: column.nodes }] : [],
    ),
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

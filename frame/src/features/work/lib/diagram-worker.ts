import type { ElkExtendedEdge, ElkNode, ElkPort, LayoutOptions } from "elkjs/lib/elk-api";
import elkScript from "elkjs/lib/elk.bundled.js?url";
import type { CanvasPosition } from "./canvas-model";
import { clean, tiersOf, type DiagramFlow, type DiagramLayout, type DiagramShape } from "./diagram";
import { DIAGRAM, gutterOf } from "./diagram-metrics";
import { diagramRows, type DiagramRows } from "./diagram-rows";
import { channels } from "./diagram-channels";

/**
 * The layered algorithm as a diagram reads: rows top to bottom as given,
 * parts ordered to cross little (their own order breaks ties), a parent
 * centred over what it feeds, lines orthogonal. Flows that leave one port
 * share one trunk and branch; flows that reach one port gather into one.
 */
export const ELK_OPTIONS: LayoutOptions = {
  "elk.algorithm": "layered",
  "elk.direction": "DOWN",
  "elk.edgeRouting": "ORTHOGONAL",
  "elk.layered.layering.strategy": "INTERACTIVE",
  "elk.layered.crossingMinimization.strategy": "LAYER_SWEEP",
  "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
  "elk.layered.thoroughness": "12",
  "elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
  "elk.layered.nodePlacement.bk.fixedAlignment": "BALANCED",
  "elk.layered.nodePlacement.favorStraightEdges": "true",
  "elk.spacing.nodeNode": String(DIAGRAM.column),
  "elk.layered.spacing.nodeNodeBetweenLayers": String(DIAGRAM.row),
  "elk.layered.spacing.edgeNodeBetweenLayers": "26",
  "elk.spacing.edgeNode": "18",
  "elk.spacing.edgeEdge": "14",
  "elk.layered.spacing.edgeEdgeBetweenLayers": "12",
  "elk.spacing.portPort": "16",
  "elk.portAlignment.default": "CENTER",
  "elk.separateConnectedComponents": "false",
  "elk.padding": "[top=0,left=0,bottom=0,right=0]",
  "elk.randomSeed": "1",
};

/**
 * How each flow is laid out. A flow to the next row is the engine's: flows
 * down leave their part through one shared port, a trunk that branches, and
 * gather into their target's shared port. A flow drawn up (`back`), and one of
 * an opposite pair, runs on a line of its own. A flow that reaches past the
 * next row (`outer`) is no part of the engine's graph: it runs down a channel
 * outside the parts (see `diagram-channels`).
 */
export type Route = {
  lead: number;
  partner?: number;
  back: boolean;
  source: boolean;
  target: boolean;
  outer: boolean;
  /** Drawn up into a part that other flows also return to: they share its way in. */
  returns?: boolean;
};

function plan(shape: DiagramShape, rows: DiagramRows): Route[] {
  const routes: Route[] = [];
  const open = new Map<string, Route>();
  const row = (id: string) => rows.rowOf.get(id)!;
  for (const flow of shape.flows) {
    const partner = open.get(`${flow.to}\n${flow.from}`);
    if (partner) {
      partner.partner = flow.index;
      partner.source = partner.target = false;
      open.delete(`${flow.to}\n${flow.from}`);
      continue;
    }
    const back = !rows.forward.get(flow.index);
    const outer = Math.abs(row(flow.to) - row(flow.from)) > 1;
    const route: Route = { lead: flow.index, back, source: !back, target: !back, outer };
    routes.push(route);
    open.set(`${flow.from}\n${flow.to}`, route);
  }
  const returning = new Map<string, Route[]>();
  for (const route of routes)
    if (route.back && route.partner === undefined && !route.outer) {
      const to = shape.flows.find((entry) => entry.index === route.lead)!.to;
      returning.set(to, [...(returning.get(to) ?? []), route]);
    }
  for (const list of returning.values())
    if (list.length > 1) for (const route of list) route.returns = true;
  return routes;
}

/** The ELK graph for a diagram: its parts in their rows, their shared and own ports, the flows. */
export function elkGraph(shape: DiagramShape, rows: DiagramRows = diagramRows(shape)): ElkNode {
  const node = new Map(shape.parts.map((part, index) => [part.id, `n${index}`]));
  const flow = new Map(shape.flows.map((entry) => [entry.index, entry]));
  const ports = new Map<string, ElkPort[]>([...node.values()].map((id) => [id, []]));
  const port = (owner: string, key: string, side: "NORTH" | "SOUTH") => {
    const list = ports.get(owner)!;
    const name = `${owner}${key}`;
    if (!list.some((entry) => entry.id === name))
      list.push({ id: name, width: 0, height: 0, layoutOptions: { "elk.port.side": side } });
    return name;
  };
  const edges: ElkExtendedEdge[] = [];
  for (const route of plan(shape, rows)) {
    if (route.outer) continue;
    const lead = flow.get(route.lead)!;
    // Laid out down the picture: from whichever end stands higher.
    const [upper, lower] = route.back ? [lead.to, lead.from] : [lead.from, lead.to];
    const owner = node.get(upper)!;
    const source = route.source
      ? port(owner, "out", "SOUTH")
      : route.returns
        ? port(owner, "back", "SOUTH")
        : port(owner, `e${route.lead}s`, "SOUTH");
    const target = route.target
      ? port(node.get(lower)!, "in", "NORTH")
      : port(node.get(lower)!, `e${route.lead}t`, "NORTH");
    edges.push({ id: `e${route.lead}`, sources: [source], targets: [target] });
  }
  const children: ElkNode[] = rows.rows.flatMap((row, at) =>
    row.map((id, column) => ({
      id: node.get(id)!,
      x: column * 400,
      y: at * 400,
      width: DIAGRAM.node.width,
      height: DIAGRAM.node.height,
      ports: ports.get(node.get(id)!)!,
      layoutOptions: { "elk.portConstraints": "FIXED_SIDE" },
    })),
  );
  return { id: "diagram", layoutOptions: ELK_OPTIONS, children, edges };
}

const round = (value: number) => Math.round(value * 10) / 10;

/**
 * What ELK answered, read back into the diagram's own coordinates: the tiers'
 * column to the left, and extra air opened above each tier after the first
 * (between the lines that gather over its parts and the parts themselves, so
 * only straight runs grow).
 */
function fromElk(
  shape: DiagramShape,
  out: ElkNode,
  rows: DiagramRows = diagramRows(shape),
): DiagramLayout {
  const { height: H } = DIAGRAM.node;
  const left = gutterOf(shape.lanes);
  const children = new Map((out.children ?? []).map((child) => [child.id, child]));
  const raw = new Map(
    shape.parts.map((part, index) => {
      const child = children.get(`n${index}`);
      return [part.id, { x: child?.x ?? 0, y: child?.y ?? 0 }];
    }),
  );
  const gaps = [...rows.tierStart]
    .map((row) => Math.min(...rows.rows[row]!.map((id) => raw.get(id)!.y)) - 13)
    .sort((a, b) => a - b);
  const moved = (point: CanvasPosition) => ({
    x: round(point.x + left),
    y: round(point.y + gaps.filter((gap) => point.y > gap).length * DIAGRAM.tier),
  });
  const at = Object.fromEntries([...raw].map(([id, point]) => [id, moved(point)]));
  const flow = new Map(shape.flows.map((entry) => [entry.index, entry]));
  const edges = new Map((out.edges ?? []).map((edge) => [edge.id, edge]));
  const flows: Record<number, DiagramFlow> = {};
  const side = (id: string) => !!shape.parts.find((part) => part.id === id)?.side;
  const quiet = (index: number) => !rows.forward.get(index) || side(flow.get(index)!.to);
  const routes = plan(shape, rows);
  const run = (id: string) => {
    const section = edges.get(id)?.sections?.[0];
    return section ? [section.startPoint, ...(section.bendPoints ?? []), section.endPoint] : null;
  };
  const drawn = (route: Route, line: CanvasPosition[]) => {
    const paired = route.partner !== undefined;
    const lead = flow.get(route.lead)!;
    const lower = route.back ? lead.from : lead.to;
    const put = (index: number, down: boolean, twin: boolean) => {
      const entry = flow.get(index)!;
      flows[index] = {
        from: entry.from,
        to: entry.to,
        points: down ? line : [...line].reverse(),
        resting: false,
        // Both ways between two parts is one solid line with a head at each end.
        quiet: paired ? side(lower) : quiet(index),
        back: paired ? false : !rows.forward.get(index),
        ...(twin ? { twin: true } : {}),
      };
    };
    put(route.lead, !route.back, false);
    if (route.partner !== undefined) put(route.partner, route.back, true);
  };
  for (const route of routes) {
    if (route.outer) continue;
    const line = run(`e${route.lead}`);
    if (line) drawn(route, clean(line.map(moved)));
  }
  // Flows past the next row run outside the parts, one channel per part they leave.
  const outer = channels(
    shape,
    rows,
    at,
    routes.filter((route) => route.outer),
    Object.values(flows).map((entry) => entry.points),
    left,
  );
  for (const [key, point] of Object.entries(outer.at)) at[key] = point;
  for (const entry of Object.values(flows))
    entry.points = entry.points.map((point) => ({ x: point.x + outer.shift, y: point.y }));
  for (const route of routes) {
    const line = outer.lines.get(route.lead);
    if (line) drawn(route, line);
  }
  const xs = [
    ...Object.values(at).flatMap((p) => [p.x, p.x + DIAGRAM.node.width]),
    ...Object.values(flows).flatMap((entry) => entry.points.map((point) => point.x)),
  ];
  const ys = [
    ...Object.values(at).flatMap((p) => [p.y, p.y + H]),
    ...Object.values(flows).flatMap((entry) => entry.points.map((point) => point.y)),
  ];
  const width = Math.ceil(Math.max(0, ...xs));
  const height = Math.ceil(Math.max(0, ...ys));
  return {
    at,
    bounds: { x: 0, y: Math.min(0, ...ys), width, height },
    gutter: left,
    tiers: tiersOf(shape, rows, at),
    flows,
    settled: true,
  };
}

/**
 * Rows that stand over each other: straightened lines may drag a chain of
 * rows off to one side, leaving the tiers' names far from their parts.
 */
function even(rows: DiagramRows, layout: DiagramLayout): boolean {
  const centres = rows.rows.map((row) => {
    const xs = row.map((id) => layout.at[id]!.x);
    return (Math.min(...xs) + Math.max(...xs) + DIAGRAM.node.width) / 2;
  });
  const span = layout.bounds.width - layout.gutter;
  return Math.max(...centres) - Math.min(...centres) <= span * 0.3;
}

type Engine = { layout: (graph: ElkNode) => Promise<ElkNode> };
let local: Promise<Engine> | null = null;
/** ELK in a worker; `failed` settles if the page refuses it (a policy, a failed load). */
let worker: { elk: Engine; failed: Promise<never> } | null | undefined;

type ElkClass = new () => Engine;
/**
 * ELK on this thread, where a worker cannot start: its script loads once as a
 * plain same-origin script (it defines the global `ELK`). Loaded as a script,
 * not a module import, so no chunk of the page needs ELK's CommonJS wrapper.
 * Off the page (unit tests) Node loads the package itself.
 */
function inPage(): Promise<Engine> {
  local ??=
    typeof document === "undefined"
      ? import(/* @vite-ignore */ ["elkjs", "lib", "elk.bundled.js"].join("/")).then(
          (module: { default: ElkClass }) => new module.default(),
        )
      : new Promise<Engine>((resolve, reject) => {
          const script = document.createElement("script");
          script.src = elkScript;
          script.addEventListener("load", () => {
            const ELK = (globalThis as { ELK?: ElkClass }).ELK;
            if (ELK) resolve(new ELK());
            else reject(new Error("elk"));
          });
          script.addEventListener("error", () => reject(new Error("elk")));
          document.head.append(script);
        });
  return local;
}

/**
 * ELK's worker script speaks a small protocol: a numbered command in, the same
 * number with its data or error out. Spoken here directly, so this chunk needs
 * none of ELK's CommonJS wrapper.
 */
function spawn() {
  if (worker !== undefined) return worker;
  try {
    const thread = new Worker(new URL("./diagram-layout.worker.ts", import.meta.url), {
      type: "module",
    });
    const waiting = new Map<
      number,
      { resolve: (out: ElkNode) => void; reject: (e: Error) => void }
    >();
    let next = 0;
    const ask = (message: Record<string, unknown>) =>
      new Promise<ElkNode>((resolve, reject) => {
        const id = next++;
        waiting.set(id, { resolve, reject });
        thread.postMessage({ ...message, id });
      });
    thread.addEventListener("message", (event: MessageEvent<Reply>) => {
      const pending = waiting.get(event.data.id);
      waiting.delete(event.data.id);
      if (event.data.error) pending?.reject(new Error(String(event.data.error)));
      else pending?.resolve(event.data.data as ElkNode);
    });
    const failed = new Promise<never>((_, reject) =>
      thread.addEventListener("error", () => reject(new Error("worker")), { once: true }),
    );
    failed.catch(() => (worker = null));
    void ask({ cmd: "register", algorithms: ["layered"] }).catch(() => {});
    worker = {
      elk: { layout: (graph) => ask({ cmd: "layout", graph, layoutOptions: {}, options: {} }) },
      failed,
    };
  } catch {
    worker = null;
  }
  return worker;
}
type Reply = { id: number; data?: unknown; error?: unknown };

async function run(graph: ElkNode): Promise<ElkNode> {
  const thread = typeof Worker === "undefined" ? null : spawn();
  if (thread)
    try {
      return await Promise.race([thread.elk.layout(graph), thread.failed]);
    } catch (error) {
      // A refused worker hands its questions to this thread; a graph ELK rejects stays rejected.
      if (worker) throw error;
    }
  return (await inPage()).layout(graph);
}

/**
 * A diagram laid out by ELK: in a worker where the page allows one, else on
 * this thread, within a result's width where it can be.
 */
export async function engineLayout(shape: DiagramShape): Promise<DiagramLayout> {
  if (!shape.parts.length) throw new Error("empty");
  let best: DiagramLayout | null = null;
  // Straight lines first; where aligning them spreads the picture too wide, rows centred on
  // each other, then fewer parts to a row.
  const tries: [number, string][] = [
    [DIAGRAM.across, "BRANDES_KOEPF"],
    [DIAGRAM.across, "SIMPLE"],
    [DIAGRAM.across - 1, "SIMPLE"],
    [DIAGRAM.across - 2, "SIMPLE"],
  ];
  for (const [across, placement] of tries) {
    const rows = diagramRows(shape, across);
    const graph = elkGraph(shape, rows);
    graph.layoutOptions = {
      ...graph.layoutOptions,
      "elk.layered.nodePlacement.strategy": placement,
    };
    const layout = fromElk(shape, await run(graph), rows);
    // Drawn a few percent smaller reads better than a row broken in two.
    if (
      layout.bounds.width <= DIAGRAM.reach * 1.06 &&
      (placement === "SIMPLE" || even(rows, layout))
    )
      return layout;
    if (!best || layout.bounds.width < best.bounds.width) best = layout;
  }
  return best!;
}

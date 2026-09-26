import type { ElkExtendedEdge, ElkNode, ElkPort, LayoutOptions } from "elkjs/lib/elk-api";
import elkScript from "elkjs/lib/elk.bundled.js?url";
import type { CanvasPosition } from "./canvas-model";
import {
  DIAGRAM,
  DIAGRAM_REACH,
  bandsOf,
  lanesOf,
  plateHeight,
  plateWidth,
  primaryFlows,
  type DiagramFlow,
  type DiagramLayout,
  type DiagramShape,
} from "./diagram";

/** The layered algorithm as a diagram reads: lanes left to right, elbows, names on their lines. */
export const ELK_OPTIONS: LayoutOptions = {
  "elk.algorithm": "layered",
  "elk.direction": "RIGHT",
  "elk.layered.crossingMinimization.strategy": "LAYER_SWEEP",
  "elk.layered.thoroughness": "10",
  "elk.layered.layering.strategy": "LONGEST_PATH",
  "elk.layered.nodePlacement.strategy": "NETWORK_SIMPLEX",
  "elk.edgeRouting": "ORTHOGONAL",
  "elk.spacing.nodeNode": "40",
  "elk.layered.spacing.nodeNodeBetweenLayers": "96",
  "elk.layered.spacing.edgeNodeBetweenLayers": "24",
  "elk.spacing.edgeNode": "16",
  "elk.spacing.edgeEdge": "12",
  "elk.layered.spacing.edgeEdgeBetweenLayers": "12",
  "elk.spacing.portPort": "12",
  "elk.edgeLabels.placement": "CENTER",
  "elk.edgeLabels.inline": "true",
  "elk.randomSeed": "1",
};
/** Room around the picture: a band's caption above and its air beside the parts. */
const LANE_PAD = { top: 28, side: 12, bottom: 12 };
/** Two opposite flows run 12 px apart; their plates stand either side, 2 px apart. */
const CORRIDOR = 6;
const PLATE_GAP = 1;

type Pair = { lead: number; back?: number; reversed: boolean };

/**
 * One ELK edge per flow, or per pair of opposite flows between the same two
 * parts, which then share one corridor. A flow runs forward through the lanes:
 * one pointing back is laid out reversed and turned round afterwards.
 */
function pairs(shape: DiagramShape): Pair[] {
  const lane = new Map(shape.parts.map((part) => [part.id, part.lane]));
  const out: Pair[] = [];
  const open = new Map<string, Pair>();
  for (const flow of shape.flows) {
    const partner = open.get(`${flow.to}\n${flow.from}`);
    if (partner) {
      partner.back = flow.index;
      open.delete(`${flow.to}\n${flow.from}`);
      continue;
    }
    const pair = { lead: flow.index, reversed: lane.get(flow.from)! > lane.get(flow.to)! };
    out.push(pair);
    open.set(`${flow.from}\n${flow.to}`, pair);
  }
  return out;
}

/** The ELK graph for a diagram: parts 220 × 72, one partition per lane, a port per flow end. */
export function elkGraph(shape: DiagramShape): ElkNode {
  const lanes = shape.lanes.length > 0;
  const lane = new Map(shape.parts.map((part) => [part.id, part.lane]));
  const node = new Map(shape.parts.map((part, index) => [part.id, `n${index}`]));
  const flow = new Map(shape.flows.map((entry) => [entry.index, entry]));
  const ports = new Map<string, ElkPort[]>(shape.parts.map((part) => [part.id, []]));
  const edges: ElkExtendedEdge[] = pairs(shape).map((pair) => {
    const lead = flow.get(pair.lead)!;
    const [from, to] = pair.reversed ? [lead.to, lead.from] : [lead.from, lead.to];
    // Within one lane a flow leaves below and lands above; across lanes it runs east to west.
    const within = lanes && lane.get(from) === lane.get(to);
    const port = (side: string) => ({
      id: `e${pair.lead}${side === "EAST" || side === "SOUTH" ? "s" : "t"}`,
      width: 0,
      height: 0,
      layoutOptions: { "elk.port.side": side },
    });
    const source = port(within ? "SOUTH" : "EAST");
    const target = port(within ? "NORTH" : "WEST");
    ports.get(from)!.push(source);
    ports.get(to)!.push(target);
    const names = [lead.label, pair.back === undefined ? "" : flow.get(pair.back)!.label].filter(
      Boolean,
    );
    const width = Math.max(0, ...names.map(plateWidth));
    const tall = Math.max(0, ...names.map(plateHeight));
    return {
      id: `e${pair.lead}`,
      sources: [source.id],
      targets: [target.id],
      ...(names.length
        ? {
            labels: [
              {
                id: `e${pair.lead}l`,
                text: names.join(" / "),
                width,
                height: pair.back === undefined ? tall : tall * 2 + PLATE_GAP * 2,
                // Read per label, not from the graph: the line runs through the plate.
                layoutOptions: { "elk.edgeLabels.inline": "true" },
              },
            ],
          }
        : {}),
    };
  });
  return {
    id: "diagram",
    layoutOptions: {
      ...ELK_OPTIONS,
      "elk.partitioning.activate": String(lanes),
      "elk.padding": lanes
        ? `[top=${LANE_PAD.top},left=${LANE_PAD.side},bottom=${LANE_PAD.bottom},right=${LANE_PAD.side}]`
        : "[top=0,left=0,bottom=0,right=0]",
    },
    children: shape.parts.map((part) => ({
      id: node.get(part.id)!,
      width: DIAGRAM.node.width,
      height: DIAGRAM.node.height,
      ports: ports.get(part.id)!,
      layoutOptions: {
        "elk.portConstraints": "FIXED_SIDE",
        ...(lanes ? { "elk.partitioning.partition": String(part.lane) } : {}),
      },
    })),
    edges,
  };
}

/** A line with no repeated or collinear points. */
function clean(points: CanvasPosition[]): CanvasPosition[] {
  const out: CanvasPosition[] = [];
  for (const point of points) {
    const last = out.at(-1);
    if (last && Math.abs(last.x - point.x) < 0.01 && Math.abs(last.y - point.y) < 0.01) continue;
    const before = out.at(-2);
    if (
      before &&
      last &&
      ((Math.abs(before.x - last.x) < 0.01 && Math.abs(last.x - point.x) < 0.01) ||
        (Math.abs(before.y - last.y) < 0.01 && Math.abs(last.y - point.y) < 0.01))
    )
      out.pop();
    out.push(point);
  }
  return out;
}

/** The left-hand normal of a segment: up for a segment running right. */
function normal(a: CanvasPosition, b: CanvasPosition): CanvasPosition {
  const run = Math.hypot(b.x - a.x, b.y - a.y) || 1;
  return { x: (b.y - a.y) / run, y: -(b.x - a.x) / run };
}

/** An orthogonal line moved `by` to its left, corner for corner. */
export function offsetLine(points: readonly CanvasPosition[], by: number): CanvasPosition[] {
  return points.map((point, i) => {
    const before = i > 0 ? normal(points[i - 1]!, point) : null;
    const after = i < points.length - 1 ? normal(point, points[i + 1]!) : null;
    const nx = (before?.x ?? 0) + (after?.x ?? 0);
    const ny = (before?.y ?? 0) + (after?.y ?? 0);
    // A corner moves along both normals; an end along its one segment's.
    return { x: point.x + nx * by, y: point.y + ny * by };
  });
}

const round = (value: number) => Math.round(value * 10) / 10;
const shift = (point: CanvasPosition, by: CanvasPosition) => ({
  x: round(point.x - by.x),
  y: round(point.y - by.y),
});

/** What ELK answered, read back into the diagram's own coordinates. */
export function fromElk(shape: DiagramShape, out: ElkNode): DiagramLayout {
  const lanes = shape.lanes.length > 0;
  const band = lanes ? DIAGRAM.layer : 0;
  const children = new Map((out.children ?? []).map((child) => [child.id, child]));
  const raw = new Map(
    shape.parts.map((part, index) => {
      const child = children.get(`n${index}`);
      return [part.id, { x: child?.x ?? 0, y: child?.y ?? 0 }];
    }),
  );
  // The first part's corner sits under the caption band, as the column layout's does.
  const corner = {
    x: Math.min(...[...raw.values()].map((p) => p.x)),
    y: Math.min(...[...raw.values()].map((p) => p.y)) - band,
  };
  const at = Object.fromEntries([...raw].map(([id, point]) => [id, shift(point, corner)]));
  const flow = new Map(shape.flows.map((entry) => [entry.index, entry]));
  const edges = new Map((out.edges ?? []).map((edge) => [edge.id, edge]));
  const primary = primaryFlows(shape);
  const flows: Record<number, DiagramFlow> = {};
  for (const pair of pairs(shape)) {
    const edge = edges.get(`e${pair.lead}`);
    const section = edge?.sections?.[0];
    if (!section) continue;
    const line = clean([section.startPoint, ...(section.bendPoints ?? []), section.endPoint]);
    const label = edge.labels?.[0];
    const centre = label
      ? { x: (label.x ?? 0) + (label.width ?? 0) / 2, y: (label.y ?? 0) + (label.height ?? 0) / 2 }
      : undefined;
    const drawn = (index: number, points: CanvasPosition[], forward: boolean, plate?: number) => {
      const entry = flow.get(index)!;
      const along = forward ? points : [...points].reverse();
      flows[index] = {
        from: entry.from,
        to: entry.to,
        points: along.map((point) => shift(point, corner)),
        ...(entry.label && centre
          ? { plate: shift(plateAt(line, centre, entry.label, plate), corner) }
          : {}),
        primary: primary.has(index),
      };
    };
    if (pair.back === undefined) {
      drawn(pair.lead, line, !pair.reversed);
      continue;
    }
    // The lead keeps left of the line ELK drew, its partner right, each with its plate.
    drawn(pair.lead, offsetLine(line, CORRIDOR), !pair.reversed, 1);
    drawn(pair.back, offsetLine(line, -CORRIDOR), pair.reversed, -1);
  }
  const box = { x: 0, y: 0, width: out.width ?? 0, height: out.height ?? 0 };
  const bounds = { ...shift(box, corner), width: box.width, height: box.height };
  return {
    at,
    width: Math.min(DIAGRAM_REACH, Math.ceil(bounds.x + bounds.width)),
    height: Math.min(DIAGRAM_REACH, Math.ceil(bounds.y + bounds.height)),
    bounds,
    layers: lanesOf(shape),
    bands: bandsOf(shape, at, bounds, LANE_PAD.side),
    flows,
    plates: {},
    routes: {},
    settled: true,
  };
}

/**
 * A plate's centre: the label's own for a single flow; for one of a pair, its
 * half of the shared label, beside the line on the side (`1` left, `-1`
 * right of the drawn direction) its flow runs.
 */
function plateAt(
  line: readonly CanvasPosition[],
  centre: CanvasPosition,
  label: string,
  side?: number,
): CanvasPosition {
  if (!side) return centre;
  const index = line.findIndex((point, i) => {
    const next = line[i + 1];
    if (!next) return false;
    const [lx, hx] = [Math.min(point.x, next.x) - 1, Math.max(point.x, next.x) + 1];
    const [ly, hy] = [Math.min(point.y, next.y) - 1, Math.max(point.y, next.y) + 1];
    return centre.x >= lx && centre.x <= hx && centre.y >= ly && centre.y <= hy;
  });
  const n = index < 0 ? { x: 0, y: -1 } : normal(line[index]!, line[index + 1]!);
  const reach = (n.y ? plateHeight(label) : plateWidth(label)) / 2 + PLATE_GAP;
  return { x: centre.x + n.x * reach * side, y: centre.y + n.y * reach * side };
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

/** A diagram laid out by ELK: in a worker where the page allows one, else on this thread. */
export async function engineLayout(shape: DiagramShape): Promise<DiagramLayout> {
  if (!shape.parts.length) throw new Error("empty");
  return fromElk(shape, await run(elkGraph(shape)));
}

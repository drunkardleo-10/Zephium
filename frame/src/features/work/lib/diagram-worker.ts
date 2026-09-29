import type { ElkExtendedEdge, ElkNode, LayoutOptions } from "elkjs/lib/elk-api";
import elkScript from "elkjs/lib/elk.bundled.js?url";
import type { CanvasPosition } from "./canvas-model";
import {
  clean,
  tiersOf,
  type DiagramFlow,
  type DiagramLayout,
  type DiagramShape,
  type DiagramWay,
} from "./diagram";
import { DIAGRAM, PLATE, ROUTE, gutterOf, plateHeight, plateWidth } from "./diagram-metrics";
import { diagramRows, type DiagramRows } from "./diagram-rows";
import { joined, passes, planOf, routeAir, topOf, type Step, type Tree } from "./diagram-route";

/**
 * The layered algorithm places the parts: rows as given, parts ordered to
 * cross little (their own order breaks ties), then placed across their rows
 * for the fewest turns (network simplex straightens more lines than
 * Brandes–Köpf on real diagrams), a trunk kept straight through the rows it
 * passes. Its lines are not
 * used: the router draws them (see `diagram-route`).
 */
export const ELK_OPTIONS: LayoutOptions = {
  "elk.algorithm": "layered",
  "elk.direction": "DOWN",
  "elk.edgeRouting": "ORTHOGONAL",
  "elk.layered.layering.strategy": "INTERACTIVE",
  "elk.layered.crossingMinimization.strategy": "LAYER_SWEEP",
  "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
  "elk.layered.thoroughness": "12",
  "elk.layered.nodePlacement.strategy": "NETWORK_SIMPLEX",
  "elk.layered.nodePlacement.bk.fixedAlignment": "BALANCED",
  "elk.layered.nodePlacement.favorStraightEdges": "true",
  "elk.spacing.nodeNode": String(DIAGRAM.column),
  "elk.layered.spacing.nodeNodeBetweenLayers": "40",
  "elk.separateConnectedComponents": "false",
  "elk.padding": "[top=0,left=0,bottom=0,right=0]",
  "elk.randomSeed": "1",
};

const { width: W, height: H } = DIAGRAM.node;
/** A part's extent across its row and along the way the picture reads. */
const extent = (way: DiagramWay) =>
  way === "down" ? { across: W, along: H } : { across: H, along: W };

/** The air between a part and one set beside it: room for the flow's name on the line across. */
function besideGap(way: DiagramWay, label: string): number {
  if (!label) return 40;
  return way === "down"
    ? Math.min(PLATE.max + 32, Math.max(40, plateWidth(label) + 28))
    : Math.max(40, plateHeight(label) + 26);
}

/** The flow into a part set beside another, and how far across its block each part stands. */
function blocks(shape: DiagramShape, rows: DiagramRows, way: DiagramWay) {
  const { across } = extent(way);
  const label = (from: string, to: string) =>
    shape.flows.find((flow) => flow.from === from && flow.to === to)?.label ?? "";
  const out = new Map<string, { width: number; offset: number; before?: number; after?: number }>();
  for (const id of rows.rows.flat()) {
    const sides = rows.beside.get(id);
    const before = sides?.before ? besideGap(way, label(id, sides.before)) : 0;
    const after = sides?.after ? besideGap(way, label(id, sides.after)) : 0;
    const offset = sides?.before ? across + before : 0;
    out.set(id, {
      width: offset + across + (sides?.after ? after + across : 0),
      offset,
      ...(sides?.before ? { before } : {}),
      ...(sides?.after ? { after } : {}),
    });
  }
  return out;
}

const unitOf = (shape: DiagramShape) => {
  const index = new Map(shape.parts.map((part, at) => [part.id, `n${at}`]));
  return (id: string) => index.get(id)!;
};
const passOf = (tree: number, row: number) => `t${tree}r${row}`;

/**
 * The ELK graph for a diagram, read down its rows whichever way it will be
 * drawn: each part as a block with the parts set beside it, a small node
 * wherever a trunk passes a row, and each tree's steps from row to row.
 */
export function elkGraph(
  shape: DiagramShape,
  rows: DiagramRows = diagramRows(shape),
  way: DiagramWay = "down",
): ElkNode {
  const { across, along } = extent(way);
  const unit = unitOf(shape);
  const sized = blocks(shape, rows, way);
  const plan = planOf(shape, rows);
  const children: ElkNode[] = [];
  const port = (id: string, x: number, height: number) => [
    { id: `${id}in`, x, y: 0, width: 0, height: 0, layoutOptions: { "elk.port.side": "NORTH" } },
    {
      id: `${id}out`,
      x,
      y: height,
      width: 0,
      height: 0,
      layoutOptions: { "elk.port.side": "SOUTH" },
    },
  ];
  const trunks = plan.trees.map((tree) => passes(tree, rows));
  // Each row in the order its units should first stand: a trunk beside the part it came from.
  const key = new Map<string, number>();
  rows.rows.forEach((row, at) => {
    const units: { node: ElkNode; key: number }[] = row.map((id, column) => {
      const block = sized.get(id)!;
      key.set(unit(id), (column + 0.5) / row.length);
      return {
        key: key.get(unit(id))!,
        node: {
          id: unit(id),
          width: block.width,
          height: along,
          ports: port(unit(id), block.offset + across / 2, along),
          layoutOptions: { "elk.portConstraints": "FIXED_POS" },
        },
      };
    });
    trunks.forEach((list, tree) => {
      if (!list.includes(at)) return;
      const above = list.includes(at - 1)
        ? passOf(tree, at - 1)
        : unit(plan.trees[tree]!.roots[0]!);
      key.set(passOf(tree, at), (key.get(above) ?? 1) + 0.001 * (tree + 1));
      units.push({
        key: key.get(passOf(tree, at))!,
        node: {
          id: passOf(tree, at),
          width: ROUTE.pass,
          height: along,
          ports: port(passOf(tree, at), ROUTE.pass / 2, along),
          layoutOptions: { "elk.portConstraints": "FIXED_POS" },
        },
      });
    });
    units
      .sort((a, b) => a.key - b.key)
      .forEach((entry, column) => children.push({ ...entry.node, x: column * 400, y: at * 400 }));
  });
  const edges: ElkExtendedEdge[] = [];
  plan.trees.forEach((tree, index) => {
    const top = topOf(tree, rows);
    const through = new Set(trunks[index]);
    const deepest = Math.max(...tree.leaves.map((leaf) => rows.rowOf.get(leaf.part)!));
    for (let row = top + 1; row <= deepest; row += 1) {
      const from = [
        ...(through.has(row - 1) ? [passOf(index, row - 1)] : []),
        ...tree.roots.filter((id) => rows.rowOf.get(id) === row - 1).map(unit),
      ];
      const to = [
        ...new Set(
          tree.leaves
            .filter((leaf) => rows.rowOf.get(leaf.part) === row)
            .map((leaf) => unit(leaf.part)),
        ),
        ...(through.has(row) ? [passOf(index, row)] : []),
      ];
      for (const source of from)
        for (const target of to)
          edges.push({
            id: `s${index}_${source}_${target}`,
            sources: [`${source}out`],
            targets: [`${target}in`],
          });
    }
  });
  return { id: "diagram", layoutOptions: ELK_OPTIONS, children, edges };
}

const round = (value: number) => Math.round(value * 10) / 10;

/**
 * What ELK placed, routed: where each part stands across its row, the air
 * between rows sized for the buses that cross it, every tree's lines, then
 * the whole turned to the way the picture reads and moved clear of the
 * tiers' names.
 */
function fromElk(
  shape: DiagramShape,
  out: ElkNode,
  rows: DiagramRows,
  way: DiagramWay,
): { layout: DiagramLayout; even: boolean } {
  const { across, along } = extent(way);
  const unit = unitOf(shape);
  const sized = blocks(shape, rows, way);
  const plan = planOf(shape, rows);
  const placed = new Map((out.children ?? []).map((child) => [child.id, child]));
  const row = (id: string) => rows.rowOf.get(id)!;

  // Across: each part's start, and each trunk where it passes a row.
  const u = new Map<string, number>();
  for (const id of rows.rows.flat()) {
    const block = sized.get(id)!;
    const x = placed.get(unit(id))?.x ?? 0;
    u.set(id, x + block.offset);
    const sides = rows.beside.get(id);
    if (sides?.before) u.set(sides.before, x);
    if (sides?.after) u.set(sides.after, x + block.offset + across + block.after!);
  }
  const trunk = (tree: number, at: number) => {
    const child = placed.get(passOf(tree, at));
    return (child?.x ?? 0) + ROUTE.pass / 2;
  };
  const offset = ROUTE.port * across;
  const port = (tree: Tree, id: string) =>
    u.get(id)! + across / 2 + (tree.kind === "back" ? offset : tree.kind === "pair" ? -offset : 0);

  // Each tree's steps across each air, and the lines the air draws for them.
  const portKey = (tree: Tree, id: string) =>
    `p${tree.kind === "down" ? "c" : tree.kind === "back" ? "b" : "a"}|${id}`;
  const trunkKey = (tree: number) => `t${tree}`;
  const steps = new Map<number, Step[]>();
  plan.trees.forEach((tree, index) => {
    const top = topOf(tree, rows);
    const through = new Set(passes(tree, rows));
    const deepest = Math.max(...tree.leaves.map((leaf) => row(leaf.part)));
    for (let gap = top; gap < deepest; gap += 1) {
      const drops = new Map<string, number>();
      for (const leaf of tree.leaves)
        if (row(leaf.part) === gap + 1) drops.set(portKey(tree, leaf.part), port(tree, leaf.part));
      if (through.has(gap + 1)) drops.set(trunkKey(index), trunk(index, gap + 1));
      const stems = [
        ...(gap === top ? [] : [trunk(index, gap)]),
        ...tree.roots.filter((id) => row(id) === gap).map((id) => port(tree, id)),
      ];
      steps.set(gap, [
        ...(steps.get(gap) ?? []),
        { tree: index, stems, drops: [...drops].map(([key, at]) => ({ key, at })) },
      ]);
    }
  });
  // A port only one line reaches takes it straight where its stem already stands over the part.
  const landing = new Map<string, number>();
  const arrivals = new Map<string, number>();
  for (const list of steps.values())
    for (const step of list)
      for (const drop of step.drops) arrivals.set(drop.key, (arrivals.get(drop.key) ?? 0) + 1);
  for (const list of steps.values())
    for (const step of list)
      for (const drop of step.drops) {
        const part = drop.key.startsWith("p") ? drop.key.slice(drop.key.indexOf("|") + 1) : "";
        if (!part || arrivals.get(drop.key) !== 1) continue;
        const start = u.get(part)!;
        const inset = Math.min(24, across / 4);
        const stem = step.stems.length === 1 ? step.stems[0]! : NaN;
        if (stem >= start + inset && stem <= start + across - inset) {
          landing.set(`${step.tree}|${drop.key}`, stem);
          drop.at = stem;
        }
      }
  const airs = rows.rows.slice(0, -1).map((_, gap) => routeAir(steps.get(gap) ?? []));
  const label = (index: number) => shape.flows.find((flow) => flow.index === index)?.label ?? "";
  // The names that land in each air: on the branches into its next row.
  const named = new Map<number, string[]>();
  for (const tree of plan.trees)
    for (const leaf of tree.leaves) {
      const text = label(leaf.index);
      if (!text) continue;
      const gap = row(leaf.part) - 1;
      named.set(gap, [...(named.get(gap) ?? []), text]);
    }
  const room = (gap: number) => {
    const names = named.get(gap) ?? [];
    if (way === "down")
      return names.some((text) => plateHeight(text) > PLATE.height) ? PLATE.line : 0;
    const widest = Math.max(0, ...names.map(plateWidth));
    return widest ? Math.max(0, Math.min(PLATE.max, widest) + 16 - ROUTE.drop) : 0;
  };
  const gaps = airs.map(({ levels }, gap) => {
    return levels
      ? ROUTE.stem + (levels - 1) * ROUTE.track + ROUTE.drop + room(gap)
      : ROUTE.straight + room(gap);
  });
  const starts: number[] = [];
  rows.rows.forEach((_, at) => {
    starts.push(
      at
        ? starts[at - 1]! + along + gaps[at - 1]! + (rows.tierStart.has(at) ? DIAGRAM.tier : 0)
        : 0,
    );
  });

  // Every flow's line, in the picture's own frame.
  const lines = new Map<number, CanvasPosition[]>();
  plan.trees.forEach((tree, index) => {
    for (const leaf of tree.leaves) {
      const [first, end] = [row(leaf.root), row(leaf.part)];
      const line: CanvasPosition[] = [{ x: port(tree, leaf.root), y: starts[first]! + along }];
      for (let gap = first; gap < end; gap += 1) {
        const last = gap + 1 === end;
        const key = last ? portKey(tree, leaf.part) : trunkKey(index);
        const stem = gap === first ? port(tree, leaf.root) : trunk(index, gap);
        const next = last
          ? (landing.get(`${index}|${key}`) ?? port(tree, leaf.part))
          : trunk(index, gap + 1);
        const level = airs[gap]!.level.get(`${index}|${key}`);
        if (level !== null && level !== undefined) {
          const y = starts[gap]! + along + ROUTE.stem + level * ROUTE.track;
          line.push({ x: stem, y }, { x: next, y });
        }
        line.push({ x: next, y: starts[gap + 1]! });
        if (!last) line.push({ x: next, y: starts[gap + 1]! + along });
      }
      const flow = shape.flows.find((entry) => entry.index === leaf.index)!;
      const drawn = clean(line);
      // Drawn from where the flow starts: a reply, or a pair led upward, runs back up.
      lines.set(leaf.index, flow.from === leaf.root ? drawn : drawn.reverse());
    }
  });
  for (const flow of plan.lateral) {
    const [a, b] = [u.get(flow.from)!, u.get(flow.to)!];
    const y = starts[row(flow.from)]! + along / 2;
    lines.set(
      flow.index,
      a < b
        ? [
            { x: a + across, y },
            { x: b, y },
          ]
        : [
            { x: a, y },
            { x: b + across, y },
          ],
    );
  }
  const keys = [...lines.keys()];
  const split = joined(keys.map((key) => lines.get(key)!));
  keys.forEach((key, at) => lines.set(key, split[at]!));

  // Turned to the way the picture reads, clear of the tiers' names.
  const gutter = way === "down" ? gutterOf(shape.lanes) : shape.lanes.length ? ROUTE.header : 0;
  const turn = (point: CanvasPosition): CanvasPosition =>
    way === "down"
      ? { x: round(point.x + gutter), y: round(point.y) }
      : { x: round(point.y), y: round(point.x + gutter) };
  const at: Record<string, CanvasPosition> = {};
  for (const [id, start] of u) {
    const corner = turn({ x: start, y: starts[row(id)]! });
    at[id] = corner;
  }
  const side = (id: string) => !!shape.parts.find((part) => part.id === id)?.side;
  const flows: Record<number, DiagramFlow> = {};
  const twinOf = new Map([...plan.twins].map(([twin, lead]) => [lead, twin]));
  const kindOf = new Map(
    plan.trees.flatMap((tree) => tree.leaves.map((leaf) => [leaf.index, tree] as const)),
  );
  for (const flow of shape.flows) {
    const line = lines.get(flow.index);
    if (!line) continue;
    const tree = kindOf.get(flow.index);
    const points = line.map(turn);
    const back = tree?.kind === "back";
    const quiet = tree?.kind === "pair" ? side(tree.leaves[0]!.part) : back || side(flow.to);
    flows[flow.index] = { from: flow.from, to: flow.to, points, resting: false, quiet, back };
    const twin = twinOf.get(flow.index);
    if (twin !== undefined) {
      const other = shape.flows.find((entry) => entry.index === twin)!;
      flows[twin] = {
        from: other.from,
        to: other.to,
        points: [...points].reverse(),
        resting: false,
        quiet,
        back: false,
        twin: true,
      };
    }
  }
  const size = way === "down" ? { w: W, h: H } : { w: W, h: H };
  const xs = [
    ...Object.values(at).map((p) => p.x + size.w),
    ...Object.values(flows).flatMap((entry) => entry.points.map((point) => point.x)),
  ];
  const ys = [
    ...Object.values(at).map((p) => p.y + size.h),
    ...Object.values(flows).flatMap((entry) => entry.points.map((point) => point.y)),
  ];
  // Rows that stand over each other: straightened trunks may drag a chain of rows aside.
  const centres = rows.rows.map((ids) => {
    const starts = ids.map((id) => u.get(id)!);
    return (Math.min(...starts) + Math.max(...starts) + across) / 2;
  });
  const all = [...u.values()];
  const span = Math.max(...all) + across - Math.min(...all);
  return {
    layout: {
      way,
      at,
      bounds: {
        x: 0,
        y: 0,
        width: Math.ceil(Math.max(0, ...xs)),
        height: Math.ceil(Math.max(0, ...ys)),
      },
      gutter,
      tiers: tiersOf(shape, rows, starts, along),
      flows,
      settled: true,
    },
    even: Math.max(...centres) - Math.min(...centres) <= span * 0.3,
  };
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

/** The ways a picture is tried: parts to a row, and how ELK places them. */
const TRIES: Record<DiagramWay, [number, string][]> = {
  down: [
    [DIAGRAM.across, "NETWORK_SIMPLEX"],
    [DIAGRAM.across, "BRANDES_KOEPF"],
    [DIAGRAM.across, "SIMPLE"],
    [DIAGRAM.across - 1, "SIMPLE"],
    [DIAGRAM.across - 2, "SIMPLE"],
  ],
  right: [
    [DIAGRAM.across, "NETWORK_SIMPLEX"],
    [DIAGRAM.across, "SIMPLE"],
  ],
};
/** Drawn a few percent smaller reads better than a row broken in two. */
const fits = (layout: DiagramLayout) => layout.bounds.width <= DIAGRAM.reach * 1.06;

/**
 * What a layout costs a reader: the length of line drawn (a shared run counted
 * once), each turn, and each crossing of two lines far more.
 */
function inkOf(layout: DiagramLayout): number {
  const runs = new Map<string, [number, number][]>();
  const segments: [CanvasPosition, CanvasPosition, number][] = [];
  const turns = new Set<string>();
  Object.values(layout.flows).forEach((flow, index) => {
    if (flow.twin) return;
    const points = flow.points;
    points.slice(1).forEach((b, at) => {
      const a = points[at]!;
      const flat = Math.abs(a.y - b.y) < 0.5;
      const key = flat ? `h${Math.round(a.y)}` : `v${Math.round(a.x)}`;
      const span: [number, number] = flat
        ? [Math.min(a.x, b.x), Math.max(a.x, b.x)]
        : [Math.min(a.y, b.y), Math.max(a.y, b.y)];
      runs.set(key, [...(runs.get(key) ?? []), span]);
      segments.push([a, b, index]);
      if (at > 0) {
        const before = points[at - 1]!;
        if (Math.abs(before.y - a.y) < 0.5 !== flat)
          turns.add(`${Math.round(a.x)},${Math.round(a.y)}`);
      }
    });
  });
  let length = 0;
  for (const spans of runs.values()) {
    spans.sort((x, y) => x[0] - y[0]);
    let [from, to] = spans[0]!;
    for (const [lo, hi] of spans.slice(1)) {
      if (lo > to) {
        length += to - from;
        [from, to] = [lo, hi];
      } else to = Math.max(to, hi);
    }
    length += to - from;
  }
  const crossings = new Set<string>();
  for (const [a, b, one] of segments) {
    if (Math.abs(a.y - b.y) >= 0.5) continue;
    for (const [c, d, other] of segments) {
      if (other === one || Math.abs(c.x - d.x) >= 0.5) continue;
      const inside =
        c.x > Math.min(a.x, b.x) + 1 &&
        c.x < Math.max(a.x, b.x) - 1 &&
        a.y > Math.min(c.y, d.y) + 1 &&
        a.y < Math.max(c.y, d.y) - 1;
      if (inside) crossings.add(`${Math.round(c.x)},${Math.round(a.y)}`);
    }
  }
  return length + turns.size * 40 + crossings.size * 240;
}

async function laidOut(shape: DiagramShape, way: DiagramWay): Promise<DiagramLayout> {
  let best: DiagramLayout | null = null;
  // Fewest turns first; where that spreads the picture too wide or drags rows aside, rows
  // centred on each other, then fewer parts to a row. Each is laid out with the parts'
  // own order held and free, and the one with less ink kept.
  for (const [across, placement] of TRIES[way]) {
    const rows = diagramRows(shape, across);
    let chosen: { layout: DiagramLayout; even: boolean; ink: number } | null = null;
    for (const held of [false, true]) {
      const graph = elkGraph(shape, rows, way);
      graph.layoutOptions = {
        ...graph.layoutOptions,
        "elk.layered.nodePlacement.strategy": placement,
        "elk.layered.crossingMinimization.forceNodeModelOrder": String(held),
      };
      const found = fromElk(shape, await run(graph), rows, way);
      const ink = inkOf(found.layout);
      if (!chosen || ink < chosen.ink) chosen = { ...found, ink };
    }
    const { layout, even } = chosen!;
    if ((way === "right" || fits(layout)) && (placement === "SIMPLE" || even)) return layout;
    if (!best || layout.bounds.width < best.bounds.width) best = layout;
  }
  return best!;
}

/**
 * A diagram laid out by ELK and routed: in a worker where the page allows
 * one, else on this thread. Both ways are laid out unless `way` holds it to
 * one; the picture takes the way that fits a result's width without being
 * drawn smaller, the shorter where both fit, the narrower where neither does.
 */
export async function engineLayout(shape: DiagramShape, way?: DiagramWay): Promise<DiagramLayout> {
  if (!shape.parts.length) throw new Error("empty");
  const found: DiagramLayout[] = [];
  for (const each of way ? [way] : (["down", "right"] as const))
    found.push(await laidOut(shape, each));
  const fitting = found.filter(fits);
  if (fitting.length)
    return fitting.sort(
      (a, b) =>
        a.bounds.height - b.bounds.height || Number(b.way === "right") - Number(a.way === "right"),
    )[0]!;
  return found.sort((a, b) => a.bounds.width - b.bounds.width)[0]!;
}

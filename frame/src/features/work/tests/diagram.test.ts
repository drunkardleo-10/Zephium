import { expect, test } from "vitest";
import type { ArtifactContent } from "$shared/ui/data/Artifact";
import type { WorkArtifactDataV1 } from "$shared/ipc/bindings";
import { renamedPart } from "../lib/correct";
import {
  DIAGRAM,
  PLATE,
  arrowHead,
  diagramColumns,
  diagramLayout,
  diagramNodeId,
  diagramOrigin,
  diagramResult,
  diagramShape,
  elbow,
  flowPath,
  midpoint,
  partAlong,
  plateHeight,
  plateWidth,
  primaryFlows,
  sceneShape,
} from "../lib/diagram";

type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;
const node = (id: string, layer?: string) => ({
  id,
  name: id,
  kind: "service",
  ...(layer ? { layer } : {}),
});
const edge = (from: string, to: string) => ({ from, to });

test("without layers, parts stand by their longest path from the sources", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: ["db", "api", "web", "cache", "queue", "worker"].map((id) => node(id)),
    edges: [
      edge("web", "api"),
      edge("api", "cache"),
      edge("api", "db"),
      edge("api", "queue"),
      edge("queue", "worker"),
      edge("worker", "db"),
    ],
    layers: [],
  };
  // The database is reached last through the worker, so it stands furthest right.
  expect(diagramColumns(diagram)).toEqual([
    { nodes: ["web"] },
    { nodes: ["api"] },
    { nodes: ["cache", "queue"] },
    { nodes: ["worker"] },
    { nodes: ["db"] },
  ]);
});

test("a cycle is cut where it closes, and a part with no edges stands with the sources", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("a"), node("b"), node("c"), node("lone")],
    edges: [edge("a", "b"), edge("b", "c"), edge("c", "a")],
    layers: [],
  };
  expect(diagramColumns(diagram)).toEqual([
    { nodes: ["a", "lone"] },
    { nodes: ["b"] },
    { nodes: ["c"] },
  ]);
});

test("stated layers are the columns, in their order, with unlayered parts last", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("db", "data"), node("web", "edge"), node("api", "app"), node("mail")],
    edges: [edge("web", "api"), edge("api", "db"), edge("api", "mail")],
    layers: [
      { id: "edge", name: "Edge" },
      { id: "app", name: "Application" },
      { id: "data", name: "Data" },
      { id: "empty", name: "Unused" },
    ],
  };
  expect(diagramColumns(diagram)).toEqual([
    { layer: "Edge", nodes: ["web"] },
    { layer: "Application", nodes: ["api"] },
    { layer: "Data", nodes: ["db"] },
    { nodes: ["mail"] },
  ]);
  const layout = diagramLayout(diagram);
  // 220 px cards 32 px apart where no flow is named, under a 16 px band for the layers' captions.
  expect(layout.at).toEqual({
    web: { x: 0, y: 16 },
    api: { x: 252, y: 16 },
    db: { x: 504, y: 16 },
    mail: { x: 756, y: 16 },
  });
  expect([layout.width, layout.height]).toEqual([976, 88]);
  expect(layout.layers.map((layer) => layer.name)).toEqual(["Edge", "Application", "Data"]);
});

test("within a column parts stack in the order the edges name them, 12 px apart", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("s3"), node("pg"), node("app")],
    edges: [edge("app", "pg"), edge("app", "s3")],
    layers: [],
  };
  const layout = diagramLayout(diagram);
  expect(layout.at).toEqual({ app: { x: 0, y: 0 }, pg: { x: 252, y: 0 }, s3: { x: 252, y: 84 } });
  expect(layout.height).toBe(156);
});

test("a gap is 32 px plus the widest plate named in it, so every flow's name has room", () => {
  const named = (from: string, to: string, label: string) => ({ from, to, label });
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("web"), node("api"), node("db"), node("cache")],
    edges: [
      named("web", "api", "HTTPS"),
      named("api", "db", "authenticated requests"),
      named("api", "cache", "reads"),
    ],
    layers: [],
  };
  expect(plateWidth("HTTPS")).toBe(Math.ceil(5 * 6.5 + 12));
  expect(plateWidth("x".repeat(80))).toBe(PLATE.max);
  const layout = diagramLayout(diagram);
  const first = 32 + plateWidth("HTTPS");
  const second = 32 + plateWidth("authenticated requests");
  expect(layout.at.api!.x).toBe(220 + first);
  expect(layout.at.db!.x).toBe(220 + first + 220 + second);
  expect(layout.width).toBe(3 * 220 + first + second);
  expect(layout.settled).toBe(false);
  // Each flow is an elbow from its source's right edge to its target's left, its plate on it.
  const flow = layout.flows[1]!;
  expect(flow.points[0]).toEqual({ x: layout.at.api!.x + 220, y: 36 });
  expect(flow.points.at(-1)).toEqual({ x: layout.at.db!.x, y: 36 });
  expect(flow.plate).toEqual({ x: layout.at.api!.x + 220 + second / 2, y: 36 });
});

const { width: W, height: H } = DIAGRAM.node;

test("a named flow between neighbours in a column widens their row gap and runs down between them", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("api", "app"), node("jobs", "app"), node("mail", "app")],
    edges: [{ from: "api", to: "jobs", label: "queues work" }, edge("jobs", "mail")],
    layers: [{ id: "app", name: "Application" }],
  };
  const layout = diagramLayout(diagram);
  expect(layout.width).toBe(W);
  const opened = PLATE.height + PLATE.air * 2;
  expect(layout.at.jobs!.y - (layout.at.api!.y + H)).toBe(opened);
  expect(layout.at.mail!.y - (layout.at.jobs!.y + H)).toBe(DIAGRAM.row);
  expect(layout.flows[0]!.points).toEqual([
    { x: W / 2, y: 16 + H },
    { x: W / 2, y: 16 + H + opened },
  ]);
  expect(layout.flows[0]!.plate).toEqual({ x: W / 2, y: 16 + H + opened / 2 });
  // The layer is a band around its column, the picture's whole height.
  expect(layout.bands).toEqual([
    { name: "Application", x: 0, y: 0, width: W, height: layout.height },
  ]);
});

test("a flow past a neighbour in one column goes out beside it and back", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("a", "app"), node("b", "app"), node("c", "app")],
    edges: [edge("a", "b"), edge("b", "c"), { from: "a", to: "c", label: "audits" }],
    layers: [{ id: "app", name: "Application" }],
  };
  const layout = diagramLayout(diagram);
  const gap = 32 + plateWidth("audits");
  expect(layout.width).toBe(W + gap);
  const a = layout.at.a!.y + H / 2;
  const c = layout.at.c!.y + H / 2;
  expect(layout.flows[2]!.points).toEqual([
    { x: W, y: a },
    { x: W + gap / 2, y: a },
    { x: W + gap / 2, y: c },
    { x: W, y: c },
  ]);
});

test("a long flow name takes two lines, and its row gap grows to hold them", () => {
  const long = "writes the session record and refreshes the cache";
  expect(plateHeight("HTTPS")).toBe(PLATE.height);
  expect(plateWidth(long)).toBe(PLATE.max);
  expect(plateHeight(long)).toBe(PLATE.height + PLATE.line);
  const stacked = diagramLayout({
    kind: "diagram",
    nodes: [node("api", "app"), node("db", "app")],
    edges: [{ from: "api", to: "db", label: long }],
    layers: [{ id: "app", name: "Application" }],
  });
  expect(stacked.at.db!.y - stacked.at.api!.y - H).toBe(plateHeight(long) + PLATE.air * 2);
});

test("a flow is primary when it is named and the first out of its source or into its target", () => {
  const shape = diagramShape({
    kind: "diagram",
    nodes: [node("web"), node("api"), node("db"), node("cache")],
    edges: [
      { from: "web", to: "api", label: "HTTPS" },
      { from: "api", to: "db", label: "SQL" },
      { from: "api", to: "cache", label: "reads" },
      { from: "web", to: "db", label: "direct" },
      { from: "web", to: "cache" },
      { from: "cache", to: "cache", label: "self" },
    ],
    layers: [],
  });
  // `reads` is api's second flow out but cache's first flow in; `direct` is neither first.
  expect([...primaryFlows(shape)]).toEqual([0, 1, 2]);
  expect(shape.flows.map((flow) => flow.index)).toEqual([0, 1, 2, 3, 4]);
});

test("the canvas finds a diagram's shape from its area, layers and links", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("db:main", "data"), node("web", "edge"), node("mail")],
    edges: [
      { from: "web", to: "db:main", label: " reads " },
      edge("web", "mail"),
      edge("mail", "mail"),
    ],
    layers: [
      { id: "edge", name: "Edge" },
      { id: "data", name: "Data" },
    ],
  };
  const id = (part: string) => diagramNodeId("element:7", part);
  const scene = sceneShape(
    "element:7",
    diagram.nodes.map((entry) => id(entry.id)),
    [
      { name: "Edge", members: [id("web")] },
      { name: "Data", members: [id("db:main")] },
    ],
    diagram.edges.map((entry, index) => ({
      id: `diagram-edge:element:7:${index}`,
      source: id(entry.from),
      target: id(entry.to),
      ...(entry.label ? { label: entry.label } : {}),
    })),
  );
  expect(scene).toEqual(diagramShape(diagram));
  expect(scene.parts).toEqual([
    { id: "db:main", lane: 1 },
    { id: "web", lane: 0 },
    { id: "mail", lane: 2 },
  ]);
});

test("an elbow crosses the gap between two boxes, or runs down when they share columns", () => {
  const box = (x: number, y: number) => ({ x, y, width: 100, height: 40 });
  expect(elbow(box(0, 0), box(200, 100))).toEqual([
    { x: 100, y: 20 },
    { x: 150, y: 20 },
    { x: 150, y: 120 },
    { x: 200, y: 120 },
  ]);
  expect(elbow(box(0, 0), box(20, 100))).toEqual([
    { x: 50, y: 40 },
    { x: 50, y: 70 },
    { x: 70, y: 70 },
    { x: 70, y: 100 },
  ]);
  expect(
    midpoint([
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 10 },
    ]),
  ).toEqual({ x: 10, y: 0 });
});

test("a flow's line turns its corners on 8 px arcs and stops short for its arrowhead", () => {
  const line = [
    { x: 0, y: 0 },
    { x: 40, y: 0 },
    { x: 40, y: 40 },
  ];
  expect(flowPath(line, 5)).toBe("M 0,0 L 32,0 Q 40,0 40,8 L 40,35");
  // A short segment takes a smaller corner.
  expect(
    flowPath([
      { x: 0, y: 0 },
      { x: 6, y: 0 },
      { x: 6, y: 20 },
    ]),
  ).toBe("M 0,0 L 3,0 Q 6,0 6,3 L 6,20");
  expect(arrowHead(line, 5)).toBe("M 40,40 L 36.5,35 L 43.5,35 Z");
});

test("a picture stands where most of its parts agree, and arrows walk along flows", () => {
  const at = { a: { x: 0, y: 16 }, b: { x: 300, y: 16 }, c: { x: 300, y: 128 } };
  expect(
    diagramOrigin([
      { position: { x: 100, y: 216 }, at: at.a },
      { position: { x: 400, y: 216 }, at: at.b },
      { position: { x: 900, y: 900 }, at: at.c },
    ]),
  ).toEqual({ origin: { x: 100, y: 200 }, count: 2 });
  const joined = [
    { id: "b", at: { x: 300, y: 0 } },
    { id: "c", at: { x: 80, y: 120 } },
    { id: "d", at: { x: -300, y: 10 } },
  ];
  expect(partAlong({ x: 0, y: 0 }, joined, "ArrowRight")).toBe("b");
  expect(partAlong({ x: 0, y: 0 }, joined, "ArrowDown")).toBe("c");
  expect(partAlong({ x: 0, y: 0 }, joined, "ArrowLeft")).toBe("d");
  expect(partAlong({ x: 0, y: 0 }, joined, "ArrowUp")).toBeNull();
});

test("a part's card names its result", () => {
  expect(diagramResult(diagramNodeId("element:7", "api"))).toBe("element:7");
  expect(diagramResult("step:element:7:0")).toBeNull();
});

test("a part is renamed in its diagram; an unknown part or the same name is refused", () => {
  const data: WorkArtifactDataV1 = {
    kind: "diagram",
    nodes: [
      { id: "db", name: "Postgres", kind: "store", vendor: "postgresql.org" },
      { id: "api", name: "API", kind: "service" },
    ],
    edges: [{ from: "api", to: "db" }],
  };
  const next = renamedPart(data, "db", "  Primary database ");
  expect(next?.kind === "diagram" && next.nodes.map((entry) => entry.name)).toEqual([
    "Primary database",
    "API",
  ]);
  expect(next?.kind === "diagram" && next.nodes[0]!.vendor).toBe("postgresql.org");
  expect(renamedPart(data, "missing", "Name")).toBeNull();
  expect(renamedPart(data, "api", "API")).toBeNull();
  expect(renamedPart(data, "api", "   ")).toBeNull();
});

import { expect, test } from "vitest";
import type { ArtifactContent } from "$shared/ui/data/Artifact";
import type { WorkArtifactDataV1 } from "$shared/ipc/bindings";
import { renamedPart } from "../lib/correct";
import {
  DIAGRAM,
  PLATE,
  curveY,
  diagramColumns,
  diagramLayout,
  diagramNodeId,
  diagramResult,
  flowCurve,
  plateHeight,
  plateWidth,
  pointOn,
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
  // A plate sits in the gap before its target, on its curve.
  expect(layout.plates[0]).toEqual({ gap: first, shift: 0 });
  // Two plates in one gap that do not overlap stay on their curves.
  expect(layout.plates[1]).toEqual({ gap: second, shift: 0 });
  expect(layout.plates[2]).toEqual({ gap: second, shift: 0 });
});

test("plates that would overlap in one gap stagger by one plate's height", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("a"), node("b"), node("c"), node("d")],
    edges: [
      edge("a", "c"),
      edge("b", "d"),
      { from: "a", to: "d", label: "writes" },
      { from: "b", to: "c", label: "reads" },
    ],
    layers: [],
  };
  const layout = diagramLayout(diagram);
  // Both crossings meet at the gap's middle; the later one steps down.
  expect(layout.plates[2]).toMatchObject({ shift: 0 });
  expect(layout.plates[3]).toMatchObject({ shift: PLATE.height + PLATE.air });
  const centre = 220 + (32 + plateWidth("writes")) / 2;
  expect(curveY(220, 36, layout.at.d!.x, layout.at.d!.y + 36, centre)).toBeCloseTo(78, 0);
});

const { width: W, height: H } = DIAGRAM.node;

test("a named flow between neighbours in a column widens their row gap and sits on its connector", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("api", "app"), node("jobs", "app"), node("mail", "app")],
    edges: [{ from: "api", to: "jobs", label: "queues work" }, edge("jobs", "mail")],
    layers: [{ id: "app", name: "Application" }],
  };
  const layout = diagramLayout(diagram);
  // No gap beside the column: the plate lives between the rows.
  expect(layout.width).toBe(W);
  const opened = PLATE.height + PLATE.air * 2;
  expect(layout.at.jobs!.y - (layout.at.api!.y + H)).toBe(opened);
  // A flow without a name keeps the 12 px gap.
  expect(layout.at.mail!.y - (layout.at.jobs!.y + H)).toBe(DIAGRAM.row);
  expect(layout.routes).toEqual({ 0: "down", 1: "down" });
  expect(layout.plates[0]).toEqual({ along: 0.5 });
  // Bottom centre to top centre: the plate's centre is the middle of the gap, on the line.
  const curve = flowCurve(W / 2, 16 + H, "bottom", W / 2, layout.at.jobs!.y, "top");
  expect(pointOn(curve, 0.5)).toEqual({ x: W / 2, y: 16 + H + opened / 2 });
});

test("a named flow past a neighbour bends out beside the column and carries its plate on the line", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("a", "app"), node("b", "app"), node("c", "app")],
    edges: [edge("a", "b"), edge("b", "c"), { from: "a", to: "c", label: "audits" }],
    layers: [{ id: "app", name: "Application" }],
  };
  const layout = diagramLayout(diagram);
  const gap = 32 + plateWidth("audits");
  expect(layout.width).toBe(W + gap);
  const route = layout.routes[2];
  expect(route).toMatchObject({ beside: expect.any(Number) });
  const reach = (route as { beside: number }).beside;
  const a = layout.at.a!.y + H / 2;
  const c = layout.at.c!.y + H / 2;
  const plate = layout.plates[2] as { along: number };
  const spot = pointOn(flowCurve(W, a, "right", W, c, "right", reach), plate.along);
  // The line's far point is the middle of the gap and the plate's centre.
  expect(spot.x).toBeCloseTo(W + gap / 2, 0);
  expect(spot.y).toBeCloseTo((a + c) / 2, 5);
});

test("a long flow name takes two lines, and its row gap grows to hold them", () => {
  const long = "writes the session record and refreshes the cache";
  expect(plateHeight("HTTPS")).toBe(PLATE.height);
  expect(plateWidth(long)).toBe(PLATE.max);
  expect(plateHeight(long)).toBe(PLATE.height + PLATE.line);
  const layout = diagramLayout({
    kind: "diagram",
    nodes: [node("api"), node("db")],
    edges: [{ from: "api", to: "db", label: long }],
    layers: [],
  });
  // Across columns the plate stands in the gap, as wide as the widest plate.
  expect(layout.at.db!.x).toBe(W + 32 + PLATE.max);
  const stacked = diagramLayout({
    kind: "diagram",
    nodes: [node("api", "app"), node("db", "app")],
    edges: [{ from: "api", to: "db", label: long }],
    layers: [{ id: "app", name: "Application" }],
  });
  expect(stacked.at.db!.y - stacked.at.api!.y - H).toBe(plateHeight(long) + PLATE.air * 2);
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

import { expect, test } from "vitest";
import type { ArtifactContent } from "$shared/ui/data/Artifact";
import type { WorkArtifactDataV1 } from "$shared/ipc/bindings";
import { renamedPart } from "../lib/correct";
import {
  PLATE,
  curveY,
  diagramColumns,
  diagramLayout,
  diagramNodeId,
  diagramResult,
  plateWidth,
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
  // 200 px cards 32 px apart where no flow is named, under a 16 px band for the layers' captions.
  expect(layout.at).toEqual({
    web: { x: 0, y: 16 },
    api: { x: 232, y: 16 },
    db: { x: 464, y: 16 },
    mail: { x: 696, y: 16 },
  });
  expect([layout.width, layout.height]).toEqual([896, 80]);
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
  expect(layout.at).toEqual({ app: { x: 0, y: 0 }, pg: { x: 232, y: 0 }, s3: { x: 232, y: 76 } });
  expect(layout.height).toBe(140);
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
  expect(layout.at.api!.x).toBe(200 + first);
  expect(layout.at.db!.x).toBe(200 + first + 200 + second);
  expect(layout.width).toBe(3 * 200 + first + second);
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
  const centre = 200 + (32 + plateWidth("writes")) / 2;
  expect(curveY(200, 32, layout.at.d!.x, layout.at.d!.y + 32, centre)).toBeCloseTo(70, 0);
});

test("a flow down one column names itself in the gap beside it, never over a part", () => {
  const diagram: Diagram = {
    kind: "diagram",
    nodes: [node("api", "app"), node("jobs", "app")],
    edges: [{ from: "api", to: "jobs", label: "queues work" }],
    layers: [{ id: "app", name: "Application" }],
  };
  const layout = diagramLayout(diagram);
  const gap = 32 + plateWidth("queues work");
  // The only column gets a gap of its own to the right for the plate.
  expect(layout.width).toBe(200 + gap);
  // From its target's top centre: right by half a card and half the gap, up into the row gap.
  expect(layout.plates[0]).toEqual({ dx: 100 + gap / 2, dy: -6 });
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

import { expect, test } from "vitest";
import type { ArtifactContent } from "$shared/ui/data/Artifact";
import type { WorkArtifactDataV1 } from "$shared/ipc/bindings";
import { renamedPart } from "../lib/correct";
import { diagramColumns, diagramLayout, diagramNodeId, diagramResult } from "../lib/diagram";

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
  // 180 px cards 32 px apart, under a 16 px band for the layers' captions.
  expect(layout.at).toEqual({
    web: { x: 0, y: 16 },
    api: { x: 212, y: 16 },
    db: { x: 424, y: 16 },
    mail: { x: 636, y: 16 },
  });
  expect([layout.width, layout.height]).toEqual([816, 72]);
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
  expect(layout.at).toEqual({ app: { x: 0, y: 0 }, pg: { x: 212, y: 0 }, s3: { x: 212, y: 68 } });
  expect(layout.height).toBe(124);
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

import { expect, test } from "vitest";
import type { ArtifactContent } from "$shared/ui/data/Artifact";
import { DIAGRAM, diagramShape, layoutDiagram } from "../lib/diagram";
import { ELK_OPTIONS, elkGraph, engineLayout } from "../lib/diagram-worker";

type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;
const { width: W, height: H } = DIAGRAM.node;

/** An API fanning out to its tier and past it into data: the shape that drew combs. */
const SAAS: Diagram = {
  kind: "diagram",
  layers: [
    { id: "edge", name: "Edge" },
    { id: "app", name: "Application" },
    { id: "data", name: "Data" },
  ],
  nodes: [
    { id: "web", name: "Web app", kind: "client", layer: "edge" },
    { id: "api", name: "API", kind: "service", layer: "app" },
    { id: "auth", name: "Auth", kind: "service", layer: "app" },
    { id: "jobs", name: "Job orchestrator", kind: "service", layer: "app" },
    { id: "queue", name: "Queue", kind: "queue", layer: "app" },
    { id: "worker", name: "Worker", kind: "worker", layer: "app" },
    { id: "db", name: "Postgres", kind: "store", layer: "data" },
    { id: "cache", name: "Redis", kind: "cache", layer: "data" },
    { id: "files", name: "Object storage", kind: "storage", layer: "data" },
  ],
  edges: [
    { from: "web", to: "api", label: "HTTPS" },
    { from: "api", to: "auth", label: "Identity" },
    { from: "api", to: "jobs", label: "Start job" },
    { from: "jobs", to: "queue" },
    { from: "queue", to: "worker" },
    { from: "api", to: "db", label: "CRUD" },
    { from: "api", to: "cache", label: "Cache" },
    { from: "api", to: "files", label: "Uploads" },
    { from: "worker", to: "db", label: "Persist" },
    { from: "db", to: "api", label: "Rows" },
  ],
};

test("a diagram becomes an ELK graph read top to bottom, rows fixed, ports shared by trunks", () => {
  const graph = elkGraph(diagramShape(SAAS));
  expect(graph.layoutOptions).toMatchObject({
    ...ELK_OPTIONS,
    "elk.direction": "DOWN",
    "elk.edgeRouting": "ORTHOGONAL",
    "elk.layered.layering.strategy": "INTERACTIVE",
  });
  const api = graph.children!.find((child) => child.id === "n1")!;
  expect([api.width, api.height]).toEqual([W, H]);
  // The API's flows to the next row leave through one port.
  const south = api.ports!.filter((port) => port.layoutOptions!["elk.port.side"] === "SOUTH");
  expect(south.map((port) => port.id)).toEqual(["n1out"]);
  // Its three flows into data skip the rows between: they are no part of the engine's graph.
  expect(graph.edges!.map((edge) => edge.id)).not.toContain("e6");
});

const segments = (points: readonly { x: number; y: number }[]) =>
  points.slice(1).map((point, i) => [points[i]!, point] as const);

test("the engine's layout: no part on another, no line through a part, all within a result", async () => {
  const layout = await engineLayout(diagramShape(SAAS));
  expect(layout.settled).toBe(true);
  expect(layout.bounds.width).toBeLessThanOrEqual(1120);
  const parts = Object.entries(layout.at);
  for (const [index, [, a]] of parts.entries())
    for (const [, b] of parts.slice(index + 1))
      expect(a.x + W <= b.x || b.x + W <= a.x || a.y + H <= b.y || b.y + H <= a.y).toBe(true);
  for (const [key, flow] of Object.entries(layout.flows)) {
    for (const [a, b] of segments(flow.points)) expect(a.x === b.x || a.y === b.y).toBe(true);
    for (const [id, at] of parts) {
      if (id === flow.from || id === flow.to) continue;
      for (const [a, b] of segments(flow.points)) {
        const [lx, hx] = [Math.min(a.x, b.x), Math.max(a.x, b.x)];
        const [ly, hy] = [Math.min(a.y, b.y), Math.max(a.y, b.y)];
        const crosses = lx < at.x + W - 1 && hx > at.x + 1 && ly < at.y + H - 1 && hy > at.y + 1;
        expect(crosses, `${key} through ${id}`).toBe(false);
      }
    }
  }
  // Tiers read down the picture: every edge part above every application part, above the data.
  const y = (id: string) => layout.at[id]!.y;
  expect(y("web")).toBeLessThan(y("api"));
  expect(Math.max(y("api"), y("worker"))).toBeLessThan(Math.min(y("db"), y("cache"), y("files")));
  expect(layout.tiers.map((tier) => tier.name)).toEqual(["Edge", "Application", "Data"]);
});

test("a fan-out leaves as one trunk: its flows share their first run", async () => {
  const layout = await engineLayout(diagramShape(SAAS));
  const first = (index: number) => layout.flows[index]!.points.slice(0, 2);
  expect(first(1)).toEqual(first(2));
});

test("flows past the next row run in a channel outside the parts, one strand per part", async () => {
  const layout = await engineLayout(diagramShape(SAAS));
  const xs = Object.values(layout.at).flatMap((at) => [at.x, at.x + W]);
  const [low, high] = [Math.min(...xs), Math.max(...xs)];
  // CRUD, Cache and Uploads leave the API together and share their channel's run.
  const channel = (index: number) =>
    layout.flows[index]!.points.filter((point) => point.x < low || point.x > high).map((p) => p.x);
  for (const index of [5, 6, 7]) expect(channel(index).length).toBeGreaterThan(0);
  expect(new Set([...channel(6), ...channel(7)]).size).toBe(1);
});

test("a pair of opposite flows is one line with a head at each end", async () => {
  const layout = await engineLayout(diagramShape(SAAS));
  const crud = layout.flows[5]!;
  const rows = layout.flows[9]!;
  expect(rows.twin).toBe(true);
  expect(rows.points).toEqual([...crud.points].reverse());
  expect(crud.back || rows.back).toBe(false);
});

test("layoutDiagram answers with the engine's layout off the page", async () => {
  const layout = await layoutDiagram(SAAS);
  expect(layout.settled).toBe(true);
  expect(Object.keys(layout.flows).map(Number)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
  expect(layout.flows[0]!.resting).toBe(true);
});

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
    { from: "worker", to: "files", label: "Artifacts" },
  ],
};

test("ELK places blocks and the trunks that pass rows; its own lines are not used", () => {
  const graph = elkGraph(diagramShape(SAAS));
  expect(graph.layoutOptions).toMatchObject({
    ...ELK_OPTIONS,
    "elk.direction": "DOWN",
    "elk.layered.layering.strategy": "INTERACTIVE",
  });
  const api = graph.children!.find((child) => child.id === "n1")!;
  expect([api.width, api.height]).toEqual([W, H]);
  expect(api.ports!.map((port) => [port.id, port.x])).toEqual([
    ["n1in", W / 2],
    ["n1out", W / 2],
  ]);
  // The API's lines to the data tier pass the three rows between as one trunk (its two-way
  // line with the database as another).
  const passing = graph.children!.filter((child) => child.id.startsWith("t"));
  const trunks = new Map<string, number>();
  for (const child of passing) {
    const tree = child.id.replace(/r\d+$/u, "");
    trunks.set(tree, (trunks.get(tree) ?? 0) + 1);
  }
  expect([...trunks.values()]).toEqual([3, 3]);
});

const segments = (points: readonly { x: number; y: number }[]) =>
  points.slice(1).map((point, i) => [points[i]!, point] as const);

test("the engine's layout: no part on another, no line through a part, all within a result", async () => {
  const layout = await engineLayout(diagramShape(SAAS), "down");
  expect(layout.settled).toBe(true);
  expect(layout.way).toBe("down");
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
  const y = (id: string) => layout.at[id]!.y;
  expect(y("web")).toBeLessThan(y("api"));
  expect(Math.max(y("api"), y("worker"))).toBeLessThan(Math.min(y("db"), y("cache"), y("files")));
  expect(layout.tiers.map((tier) => tier.name)).toEqual(["Edge", "Application", "Data"]);
});

/** The runs two lines share from their start. */
const shared = (a: readonly { x: number; y: number }[], b: readonly { x: number; y: number }[]) => {
  let at = 0;
  while (at < Math.min(a.length, b.length) && a[at]!.x === b[at]!.x && a[at]!.y === b[at]!.y)
    at += 1;
  return at;
};

test("a fan-out leaves as one trunk: its flows share their first run", async () => {
  const layout = await engineLayout(diagramShape(SAAS), "down");
  expect(shared(layout.flows[1]!.points, layout.flows[2]!.points)).toBeGreaterThanOrEqual(2);
});

test("flows past the next row share one trunk down through the rows between", async () => {
  const layout = await engineLayout(diagramShape(SAAS), "down");
  // Cache and Uploads leave the API together and part only in the air above the data tier.
  const [cache, files] = [layout.flows[6]!.points, layout.flows[7]!.points];
  const together = shared(cache, files);
  expect(cache[together - 1]!.y).toBeGreaterThan(layout.at.worker!.y + H);
  // Nothing else of the API's one-way lines runs beside that trunk: one line, not a comb.
  const xs = new Set(
    [6, 7].map((index) => layout.flows[index]!.points.find((p) => p.y > layout.at.jobs!.y)!.x),
  );
  expect(xs.size).toBe(1);
});

test("flows into one part join before it", async () => {
  const layout = await engineLayout(diagramShape(SAAS), "down");
  // Uploads from the API and Artifacts from the worker end on one run into object storage.
  const [uploads, artifacts] = [layout.flows[7]!.points, layout.flows[10]!.points];
  expect(uploads.at(-1)).toEqual(artifacts.at(-1));
  expect(uploads.at(-2)!.x).toBe(artifacts.at(-2)!.x);
});

test("a trunk crosses a row between its parts, never around the row's end", async () => {
  const shape = diagramShape({
    kind: "diagram",
    layers: [
      { id: "top", name: "Top" },
      { id: "mid", name: "Middle" },
      { id: "end", name: "End" },
    ],
    nodes: [
      { id: "source", name: "Source", kind: "service", layer: "top" },
      ...["b", "c", "d", "e"].map((id) => ({ id, name: id, kind: "service", layer: "mid" })),
      { id: "far", name: "Far", kind: "store", layer: "end" },
    ],
    edges: [
      ...["b", "c", "d", "e"].map((to) => ({ from: "source", to })),
      { from: "source", to: "far" },
      { from: "e", to: "far" },
    ],
  });
  const layout = await engineLayout(shape, "down");
  const row = ["b", "c", "d", "e"].map((id) => layout.at[id]!.x);
  const top = layout.at.b!.y;
  const across = segments(layout.flows[4]!.points)
    .map(([a, b]) => ({
      x: a.x,
      lo: Math.min(a.y, b.y),
      hi: Math.max(a.y, b.y),
      flat: a.x !== b.x,
    }))
    .find((run) => !run.flat && run.lo < top && run.hi > top + H)!;
  expect(across.x).toBeGreaterThan(Math.min(...row) + W);
  expect(across.x).toBeLessThan(Math.max(...row));
});

test("a pair of opposite flows is one line with a head at each end", async () => {
  const layout = await engineLayout(diagramShape(SAAS), "down");
  const crud = layout.flows[5]!;
  const rows = layout.flows[9]!;
  expect(rows.twin).toBe(true);
  expect(rows.points).toEqual([...crud.points].reverse());
  expect(crud.back || rows.back).toBe(false);
});

test("read to the right, rows become columns and tiers are named above them", async () => {
  const layout = await engineLayout(diagramShape(SAAS), "right");
  expect(layout.way).toBe("right");
  const x = (id: string) => layout.at[id]!.x;
  expect(x("web")).toBeLessThan(x("api"));
  expect(x("worker")).toBeLessThan(x("db"));
  expect(layout.gutter).toBeGreaterThan(0);
  expect(Math.min(...Object.values(layout.at).map((at) => at.y))).toBeGreaterThanOrEqual(
    layout.gutter,
  );
  expect(layout.tiers[1]!.start).toBe(x("api"));
});

test("a picture reads to the right unless it is held to reading down, a chain on one line", async () => {
  expect((await engineLayout(diagramShape(SAAS))).way).toBe("right");
  expect((await engineLayout(diagramShape(SAAS), "down")).way).toBe("down");
  const chain = await engineLayout(
    diagramShape({
      kind: "diagram",
      layers: [],
      nodes: [
        { id: "a", name: "Source", kind: "service" },
        { id: "b", name: "Transform", kind: "worker" },
        { id: "c", name: "Sink", kind: "store" },
      ],
      edges: [
        { from: "a", to: "b" },
        { from: "b", to: "c" },
      ],
    }),
  );
  expect(chain.way).toBe("right");
  expect(new Set(Object.values(chain.at).map((at) => at.y)).size).toBe(1);
});

test("layoutDiagram answers with the engine's layout off the page", async () => {
  const layout = await layoutDiagram(SAAS);
  expect(layout.settled).toBe(true);
  expect(Object.keys(layout.flows).map(Number)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  expect(layout.flows[0]!.resting).toBe(true);
});

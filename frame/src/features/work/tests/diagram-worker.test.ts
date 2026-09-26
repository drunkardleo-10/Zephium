import { expect, test } from "vitest";
import type { ElkNode } from "elkjs/lib/elk-api";
import type { ArtifactContent } from "$shared/ui/data/Artifact";
import { DIAGRAM, diagramShape, layoutDiagram, plateHeight, plateWidth } from "../lib/diagram";
import { ELK_OPTIONS, elkGraph, engineLayout, fromElk, offsetLine } from "../lib/diagram-worker";

type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;
const { width: W, height: H } = DIAGRAM.node;

/** Five parts in two layers: a browser and a CDN at the edge, three services behind. */
const FIVE: Diagram = {
  kind: "diagram",
  layers: [
    { id: "edge", name: "Edge" },
    { id: "app", name: "Application" },
  ],
  nodes: [
    { id: "web", name: "Browser", kind: "client", layer: "edge" },
    { id: "cdn", name: "CDN", kind: "edge", layer: "edge" },
    { id: "api", name: "API", kind: "service", layer: "app" },
    { id: "jobs", name: "Jobs", kind: "worker", layer: "app" },
    { id: "db", name: "Postgres", kind: "store", layer: "app" },
  ],
  edges: [
    { from: "web", to: "api", label: "HTTPS" },
    { from: "api", to: "jobs", label: "enqueues" },
    { from: "jobs", to: "web", label: "push" },
    { from: "api", to: "db", label: "SQL" },
    { from: "db", to: "api", label: "rows" },
    { from: "web", to: "cdn" },
  ],
};

test("a diagram becomes an ELK graph: a partition per layer, a port per flow end, sized plates", () => {
  const graph = elkGraph(diagramShape(FIVE));
  expect(graph.layoutOptions).toMatchObject({
    ...ELK_OPTIONS,
    "elk.layered.layering.strategy": "INTERACTIVE",
    "elk.direction": "RIGHT",
    "elk.edgeRouting": "ORTHOGONAL",
    "elk.partitioning.activate": "true",
  });
  expect(
    graph.children!.map((child) => [
      child.width,
      child.height,
      child.layoutOptions!["elk.partitioning.partition"],
      child.layoutOptions!["elk.portConstraints"],
    ]),
  ).toEqual([
    [W, H, "0", "FIXED_SIDE"],
    [W, H, "0", "FIXED_SIDE"],
    [W, H, "1", "FIXED_SIDE"],
    [W, H, "1", "FIXED_SIDE"],
    [W, H, "1", "FIXED_SIDE"],
  ]);
  const side = (node: number, port: string) =>
    graph.children![node]!.ports!.find((entry) => entry.id === port)?.layoutOptions?.[
      "elk.port.side"
    ];
  // Across layers a flow leaves east and lands west.
  expect([side(0, "e0s"), side(2, "e0t")]).toEqual(["EAST", "WEST"]);
  // Within a layer a flow is no part of the engine's graph: it is drawn down its column after.
  expect(side(2, "e1s")).toBeUndefined();
  // A flow back to an earlier layer is laid out forward, from its target.
  expect([side(0, "e2s"), side(3, "e2t")]).toEqual(["EAST", "WEST"]);
  expect(graph.edges!.map((edge) => edge.id)).toEqual(["e0", "e2"]);
  expect(graph.edges!.find((edge) => edge.id === "e0")!.labels![0]).toMatchObject({
    width: plateWidth("HTTPS"),
    layoutOptions: { "elk.edgeLabels.inline": "true" },
  });
});

test("ELK's answer reads back as parts from the first corner, flows turned round, and bands", () => {
  const shape = diagramShape({
    kind: "diagram",
    layers: [
      { id: "a", name: "Left" },
      { id: "b", name: "Right" },
    ],
    nodes: [
      { id: "x", name: "X", kind: "service", layer: "a" },
      { id: "y", name: "Y", kind: "service", layer: "b" },
    ],
    edges: [{ from: "y", to: "x", label: "back" }],
  });
  const out: ElkNode = {
    id: "diagram",
    width: 600,
    height: 140,
    children: [
      { id: "n0", x: 12, y: 40 },
      { id: "n1", x: 368, y: 40 },
    ],
    edges: [
      {
        id: "e0",
        sources: ["e0s"],
        targets: ["e0t"],
        sections: [
          {
            id: "s",
            startPoint: { x: 232, y: 76 },
            bendPoints: [{ x: 300, y: 76 }],
            endPoint: { x: 368, y: 76 },
          },
        ],
        labels: [{ id: "l", x: 280, y: 67.5, width: 40, height: 17 }],
      },
    ],
  };
  const layout = fromElk(shape, out);
  expect(layout.settled).toBe(true);
  expect(layout.at).toEqual({ x: { x: 0, y: 16 }, y: { x: 356, y: 16 } });
  // The flow from Y back to X runs from Y's west side into X's east side.
  expect(layout.flows[0]).toEqual({
    from: "y",
    to: "x",
    points: [
      { x: 356, y: 52 },
      { x: 220, y: 52 },
    ],
    plate: { x: 288, y: 52 },
    primary: true,
  });
  expect(layout.bounds).toEqual({ x: -12, y: -24, width: 600, height: 140 });
  expect([layout.width, layout.height]).toEqual([588, 116]);
  expect(layout.bands.map((band) => [band.name, band.x, band.width])).toEqual([
    ["Left", -12, W + 24],
    ["Right", 344, W + 24],
  ]);
});

test("a line moves to its left corner for corner", () => {
  expect(
    offsetLine(
      [
        { x: 0, y: 0 },
        { x: 10, y: 0 },
        { x: 10, y: 10 },
      ],
      6,
    ),
  ).toEqual([
    { x: 0, y: -6 },
    { x: 16, y: -6 },
    { x: 16, y: 10 },
  ]);
});

test("the engine lays out five parts in two lanes, opposite flows 12 px apart with both names", async () => {
  const layout = await engineLayout(diagramShape(FIVE));
  expect(layout.settled).toBe(true);
  const x = (id: string) => layout.at[id]!.x;
  // Each layer is a vertical lane: every Edge part stands left of every Application part.
  expect(Math.max(x("web"), x("cdn")) + W).toBeLessThan(Math.min(x("api"), x("jobs"), x("db")));
  expect(layout.bands.map((band) => band.name)).toEqual(["Edge", "Application"]);
  for (const band of layout.bands) {
    expect(band.y).toBe(layout.bounds.y);
    expect(band.height).toBe(layout.bounds.height);
  }
  // Lines are orthogonal and end on their parts' sides.
  for (const flow of Object.values(layout.flows)) {
    flow.points.slice(1).forEach((point, i) => {
      const before = flow.points[i]!;
      expect(point.x === before.x || point.y === before.y).toBe(true);
    });
  }
  const sql = layout.flows[3]!;
  const rows = layout.flows[4]!;
  expect([sql.from, sql.to, rows.from, rows.to]).toEqual(["api", "db", "db", "api"]);
  // Down their column 12 px apart, the reply running the other way.
  const back = [...rows.points].reverse();
  expect(back).toHaveLength(sql.points.length);
  sql.points.forEach((point, i) => {
    const other = back[i]!;
    const dx = Math.abs(point.x - other.x);
    const dy = Math.abs(point.y - other.y);
    expect([dx, dy].every((d) => d === 0 || Math.abs(d - 12) < 0.01)).toBe(true);
    expect(dx + dy).toBeGreaterThan(0);
  });
  // Both names stand beside their own line, never on each other.
  const plate = (at: { x: number; y: number }, label: string) => ({
    left: at.x - plateWidth(label) / 2,
    right: at.x + plateWidth(label) / 2,
    top: at.y - plateHeight(label) / 2,
    bottom: at.y + plateHeight(label) / 2,
  });
  const a = plate(sql.plate!, "SQL");
  const b = plate(rows.plate!, "rows");
  expect(a.bottom <= b.top || b.bottom <= a.top || a.right <= b.left || b.right <= a.left).toBe(
    true,
  );
});

test("layoutDiagram answers with the engine's layout off the page", async () => {
  const layout = await layoutDiagram(FIVE);
  expect(layout.settled).toBe(true);
  expect(Object.keys(layout.flows).map(Number)).toEqual([0, 1, 2, 3, 4, 5]);
});

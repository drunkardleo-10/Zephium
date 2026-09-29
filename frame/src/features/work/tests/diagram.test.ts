import { expect, test } from "vitest";
import type { ArtifactContent } from "$shared/ui/data/Artifact";
import {
  DIAGRAM,
  PLATE,
  arrowHead,
  diagramLayout,
  diagramShape,
  flowPath,
  midpoint,
  partAlong,
  plateHeight,
  plateWidth,
} from "../lib/diagram";
import { roleGlyph } from "../lib/diagram-icons";
import { gutterOf } from "../lib/diagram-metrics";
import { diagramRows } from "../lib/diagram-rows";
import { routeAir } from "../lib/diagram-route";
import { placePlates } from "../lib/diagram-labels";
import ApiGatewayIcon from "@hugeicons/core-free-icons/ApiGatewayIcon";
import BucketIcon from "@hugeicons/core-free-icons/BucketIcon";
import CpuIcon from "@hugeicons/core-free-icons/CpuIcon";
import CreditCardIcon from "@hugeicons/core-free-icons/CreditCardIcon";
import CubeIcon from "@hugeicons/core-free-icons/CubeIcon";
import DatabaseIcon from "@hugeicons/core-free-icons/DatabaseIcon";
import Queue01Icon from "@hugeicons/core-free-icons/Queue01Icon";
import SmartPhone01Icon from "@hugeicons/core-free-icons/SmartPhone01Icon";
import WorkflowSquare03Icon from "@hugeicons/core-free-icons/WorkflowSquare03Icon";

type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;
const node = (id: string, layer?: string) => ({
  id,
  name: id,
  kind: "service",
  ...(layer ? { layer } : {}),
});
const edge = (from: string, to: string, label?: string) => ({
  from,
  to,
  ...(label ? { label } : {}),
});
const { width: W, height: H } = DIAGRAM.node;

test("without tiers, parts stand a row below the furthest part that flows into them", () => {
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
  expect(diagramRows(diagramShape(diagram)).rows).toEqual([
    ["web"],
    ["api"],
    ["cache", "queue"],
    ["worker"],
    ["db"],
  ]);
});

test("a cycle is cut where it closes and its closing flow is drawn back up", () => {
  const shape = diagramShape({
    kind: "diagram",
    nodes: [node("a"), node("b"), node("c"), node("lone")],
    edges: [edge("a", "b"), edge("b", "c"), edge("c", "a")],
    layers: [],
  });
  const rows = diagramRows(shape);
  expect(rows.rows).toEqual([["a", "lone"], ["b"], ["c"]]);
  expect([...rows.forward.values()]).toEqual([true, true, false]);
});

test("tiers stand in their stated order, each its own rows, unlayered parts last", () => {
  const shape = diagramShape({
    kind: "diagram",
    nodes: [
      node("db", "data"),
      node("web", "edge"),
      node("api", "app"),
      node("jobs", "app"),
      node("mail"),
    ],
    edges: [edge("web", "api"), edge("api", "db"), edge("api", "jobs"), edge("jobs", "db")],
    layers: [
      { id: "edge", name: "Edge" },
      { id: "app", name: "Application" },
      { id: "data", name: "Data" },
      { id: "empty", name: "Unused" },
    ],
  });
  const rows = diagramRows(shape);
  expect(rows.rows).toEqual([["web"], ["api"], ["jobs"], ["db"], ["mail"]]);
  expect(rows.tiers).toEqual([
    { lane: 0, first: 0, last: 0 },
    { lane: 1, first: 1, last: 2 },
    { lane: 2, first: 3, last: 3 },
    { lane: 3, first: 4, last: 4 },
  ]);
  expect([...rows.tierStart]).toEqual([1, 3, 4]);
});

test("a row wider than five parts wraps into balanced rows", () => {
  const shape = diagramShape({
    kind: "diagram",
    nodes: [node("hub"), ...["a", "b", "c", "d", "e", "f", "g"].map((id) => node(id))],
    edges: ["a", "b", "c", "d", "e", "f", "g"].map((id) => edge("hub", id)),
    layers: [],
  });
  expect(diagramRows(shape).rows).toEqual([["hub"], ["a", "b", "c", "d"], ["e", "f", "g"]]);
});

test("a leaf that would take its tier's last row alone stands beside the part feeding it", () => {
  const shape = diagramShape({
    kind: "diagram",
    nodes: [
      node("web", "edge"),
      node("api", "app"),
      node("auth", "app"),
      node("billing", "app"),
      node("db", "data"),
      node("backup", "data"),
    ],
    edges: [
      edge("web", "api"),
      edge("api", "auth", "Identity"),
      edge("api", "billing"),
      edge("api", "db"),
      edge("db", "backup"),
    ],
    layers: [
      { id: "edge", name: "Edge" },
      { id: "app", name: "Application" },
      { id: "data", name: "Data" },
    ],
  });
  const rows = diagramRows(shape);
  // The API flows on to the data, so its leaves stand beside it; a pipeline's end stays below.
  expect(rows.rows).toEqual([["web"], ["api"], ["db"], ["backup"]]);
  expect(rows.beside.get("api")).toEqual({ after: "auth", before: "billing" });
  expect(rows.rowOf.get("auth")).toBe(1);
  expect([...rows.forward.values()]).toEqual([true, true, true, true, true]);
  // A line that has to pass the API's row keeps the leaves in a row of their own.
  const passed = diagramShape({
    kind: "diagram",
    nodes: [
      ...shape.parts.map((part) =>
        node(part.id, ["edge", "app", "app", "app", "data", "data"][shape.parts.indexOf(part)]),
      ),
    ],
    edges: [...shape.flows.map((flow) => edge(flow.from, flow.to)), edge("web", "db")],
    layers: [
      { id: "edge", name: "Edge" },
      { id: "app", name: "Application" },
      { id: "data", name: "Data" },
    ],
  });
  expect(diagramRows(passed).beside.size).toBe(0);
  expect(diagramRows(passed).rows).toEqual([
    ["web"],
    ["api"],
    ["auth", "billing"],
    ["db"],
    ["backup"],
  ]);
});

test("one air: a tree's own branches run above the run trees share into the same ports", () => {
  const { levels, level } = routeAir([
    {
      tree: 0,
      stems: [300],
      drops: [
        { key: "pc|db", at: 100 },
        { key: "pc|files", at: 200 },
        { key: "pc|cache", at: 320 },
      ],
    },
    {
      tree: 1,
      stems: [150],
      drops: [
        { key: "pc|db", at: 100 },
        { key: "pc|files", at: 200 },
      ],
    },
  ]);
  // The API's cache is its own; both reach the database and files on one joined run.
  expect(levels).toBe(2);
  expect(level.get("0|pc|cache")).toBe(0);
  expect(level.get("0|pc|db")).toBe(1);
  expect(level.get("1|pc|db")).toBe(1);
  expect(level.get("1|pc|files")).toBe(1);
  // A stem straight over its only port needs no run.
  expect(routeAir([{ tree: 0, stems: [40], drops: [{ key: "pc|a", at: 40 }] }]).levels).toBe(0);
});

test("the rows' layout stands at once: tiers named beside their first row, rows centred", () => {
  const layout = diagramLayout({
    kind: "diagram",
    nodes: [node("web", "edge"), node("api", "app"), node("db", "app")],
    edges: [edge("web", "api", "HTTPS"), edge("api", "db")],
    layers: [
      { id: "edge", name: "Edge" },
      { id: "app", name: "Application" },
    ],
  });
  expect(layout.settled).toBe(false);
  const left = gutterOf(["Edge", "Application"]);
  expect(layout.gutter).toBe(left);
  expect(layout.at.web).toEqual({ x: left, y: 0 });
  expect(layout.at.api).toEqual({ x: left, y: H + DIAGRAM.row + DIAGRAM.tier });
  expect(layout.tiers.map((tier) => [tier.name, tier.start])).toEqual([
    ["Edge", 0],
    ["Application", H + DIAGRAM.row + DIAGRAM.tier],
  ]);
  // A flow runs straight down from its source's bottom to its target's top, its name on it.
  const https = layout.flows[0]!;
  expect(https.points).toEqual([
    { x: left + W / 2, y: H },
    { x: left + W / 2, y: layout.at.api!.y },
  ]);
  expect(https.resting).toBe(true);
  expect(https.plate!.x).toBe(left + W / 2);
});

test("a tier column is as wide as its longest word needs", () => {
  expect(gutterOf([])).toBe(0);
  expect(gutterOf(["Edge"])).toBe(DIAGRAM.gutter);
  expect(gutterOf(["Browser coordination"])).toBeGreaterThan(DIAGRAM.gutter);
  expect(gutterOf(["Supercalifragilisticexpialidocious"])).toBe(168);
});

test("names stand on runs a flow has to itself, and a name siblings share reads once", () => {
  const shape = diagramShape({
    kind: "diagram",
    nodes: [node("web"), node("app"), node("cdn")],
    edges: [edge("web", "cdn", "Requests"), edge("app", "cdn", "Requests")],
    layers: [],
  });
  const layout = diagramLayout({
    kind: "diagram",
    nodes: [node("web"), node("app"), node("cdn")],
    edges: [edge("web", "cdn", "Requests"), edge("app", "cdn", "Requests")],
    layers: [],
  });
  const plates = placePlates(shape, layout);
  expect(plates.get(0)!.resting).toBe(true);
  // Both into one part under one name: each reads it, at one place or the other, clear of parts.
  for (const plate of plates.values())
    for (const at of Object.values(layout.at))
      expect(
        plate.at.x + plateWidth("Requests") / 2 <= at.x ||
          plate.at.x - plateWidth("Requests") / 2 >= at.x + W ||
          plate.at.y + PLATE.height / 2 <= at.y ||
          plate.at.y - PLATE.height / 2 >= at.y + H,
      ).toBe(true);
});

test("a flow's name is sized for its words, two lines past the widest plate", () => {
  expect(plateWidth("HTTPS")).toBe(Math.ceil(5 * PLATE.char + PLATE.pad));
  expect(plateWidth("x".repeat(80))).toBe(PLATE.max);
  expect(plateHeight("HTTPS")).toBe(PLATE.height);
  expect(plateHeight("x".repeat(80))).toBe(PLATE.height + PLATE.line);
});

test("a part without a product's mark shows its role, by its name before its kind", () => {
  const glyph = (name: string, kind = "service", note?: string) =>
    roleGlyph({ name, kind, ...(note ? { note } : {}) });
  expect(glyph("Model Gateway", "gateway")).toBe(ApiGatewayIcon);
  expect(glyph("Job Orchestrator")).toBe(WorkflowSquare03Icon);
  expect(glyph("Task Queue")).toBe(Queue01Icon);
  expect(glyph("AI Workers", "worker")).toBe(CpuIcon);
  expect(glyph("Billing + Webhooks")).toBe(CreditCardIcon);
  expect(glyph("Object Storage", "storage")).toBe(BucketIcon);
  expect(glyph("Mobile Client", "client")).toBe(SmartPhone01Icon);
  expect(glyph("Primary", "store")).toBe(DatabaseIcon);
  expect(glyph("Thing", "other")).toBe(CubeIcon);
});

test("a flow's line turns its corners on 8 px arcs and stops short for its arrowhead", () => {
  const line = [
    { x: 0, y: 0 },
    { x: 40, y: 0 },
    { x: 40, y: 40 },
  ];
  expect(flowPath(line, 5)).toBe("M 0,0 L 32,0 Q 40,0 40,8 L 40,35");
  expect(
    flowPath([
      { x: 0, y: 0 },
      { x: 6, y: 0 },
      { x: 6, y: 20 },
    ]),
  ).toBe("M 0,0 L 3,0 Q 6,0 6,3 L 6,20");
  expect(arrowHead(line, 5)).toBe("M 40,40 L 36.5,35 L 43.5,35 Z");
  expect(
    midpoint([
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 10 },
    ]),
  ).toEqual({ x: 10, y: 0 });
});

test("arrows walk along flows", () => {
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

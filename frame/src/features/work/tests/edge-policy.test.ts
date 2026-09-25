import { expect, test } from "vitest";
import { relationLinks, restLink, type CanvasItem, type CanvasLink } from "../lib/canvas-model";
import { environmentClusters, environmentLinks } from "../lib/project-environment";
import { environmentRequests, type WorkStage } from "../lib/project-environment-thread";
import { snapshot } from "./environment-fixtures";
import { SIZES, laneShape, laneSlots, placeLane, type StageContents } from "../lib/stage-layout";

const one = (id: string, size: { width: number; height: number }) => ({
  members: [{ id, size }],
});
const full: StageContents = {
  sources: one("sources", SIZES.sources),
  pages: { members: [...one("page-a", SIZES.page).members, ...one("page-b", SIZES.page).members] },
  subjects: one("subject", SIZES.subject),
  findings: one("findings", SIZES.findings),
  results: one("result", SIZES.document),
  plan: { members: [{ id: "step", size: { width: 248, height: 120 }, of: "result" }] },
};

/** Lanes of one work, placed as the canvas places them. */
function lanes(...contents: StageContents[]): WorkStage[] {
  const shapes = contents.map(laneShape);
  const slots = laneSlots(shapes);
  let y = 0;
  return contents.map((entry, index) => {
    const card = index ? `request-${index}` : "request";
    const place = { x: 0, y, ...SIZES.request };
    const layout = placeLane(place, shapes[index]!, slots);
    y = layout.extent + 96;
    return {
      element: "request",
      objective: "objective",
      card,
      request: "Compare quiet keyboards",
      executions: [],
      place,
      contents: entry,
      layout,
      slots,
      live: false,
      targets: {},
      facts: {},
    };
  });
}
const ends = (links: readonly CanvasLink[]) => links.map((link) => [link.source, link.target]);

test("at rest a full lane reads request → Worked with → Found → Made, and a result into its steps", () => {
  const { clusters, links } = environmentClusters(lanes(full));
  expect(ends(links)).toEqual([
    ["request", "group:request:worked"],
    ["group:request:worked", "group:request:found"],
    ["group:request:found", "group:request:made"],
    ["result", "group:request:steps:result"],
  ]);
  expect(links.every(restLink)).toBe(true);
  expect(clusters.map((cluster) => [cluster.id, cluster.label, cluster.members])).toEqual([
    ["group:request:worked", "2 pages", ["sources", "page-a", "page-b"]],
    ["group:request:found", "1 subject · Findings", ["subject", "findings"]],
    ["group:request:steps:result", "1 step", ["step"]],
    ["group:request:made", "Result", ["result"]],
  ]);
});

test("a missing group is skipped: a pure result is one edge from its request", () => {
  expect(ends(environmentClusters(lanes({ results: one("result", SIZES.result) })).links)).toEqual([
    ["request", "group:request:made"],
  ]);
  expect(
    ends(
      environmentClusters(
        lanes({ sources: one("sources", SIZES.sources), results: one("result", SIZES.result) }),
      ).links,
    ),
  ).toEqual([
    ["request", "group:request:worked"],
    ["group:request:worked", "group:request:made"],
  ]);
});

test("two lanes rest on their own group edges and the thread joins request to request", () => {
  const stages = lanes(full, { results: one("later", SIZES.result) });
  expect(ends(environmentClusters(stages).links)).toEqual([
    ["request", "group:request:worked"],
    ["group:request:worked", "group:request:found"],
    ["group:request:found", "group:request:made"],
    ["result", "group:request:steps:result"],
    ["request-1", "group:request-1:made"],
  ]);
  const thread = environmentRequests(stages).links;
  expect(thread.map((link) => [link.kind, link.source, link.target])).toEqual([
    ["thread", "request", "request-1"],
  ]);
  // The second lane's Made sits in slot 1, under the first lane's Worked with.
  expect(stages[1]!.layout.groups[0]!.box.x).toBe(stages[0]!.layout.groups[0]!.box.x);
});

const card = (id: string, type: CanvasItem["type"]): CanvasItem => ({
  id,
  type,
  title: id,
  kind: "",
  detail: "",
  status: "",
});
const items = new Map(
  [
    card("request", "objective"),
    card("sources", "sources"),
    card("page-a", "page"),
    card("page-b", "page"),
    card("subject", "subject"),
    card("finding", "finding"),
    card("step", "step"),
    { ...card("result", "result"), layout: "artifact" as const },
    card("note", "note"),
    card("tab", "tab"),
  ].map((item) => [item.id, item]),
);
const relations: CanvasLink[] = [
  { id: "a-subject", source: "page-a", target: "subject", kind: "supports", role: "evidence" },
  { id: "b-subject", source: "page-b", target: "subject", kind: "supports", role: "evidence" },
  { id: "a-finding", source: "page-a", target: "finding", kind: "supports", role: "evidence" },
  { id: "step-subject", source: "step", target: "subject", kind: "uses", role: "named" },
  // Rust's own ties between lane cards: the group edges already say them.
  { id: "subject-result", source: "subject", target: "result", kind: "uses" },
  { id: "finding-subject", source: "finding", target: "subject", kind: "supports" },
  { id: "note-tab", source: "note", target: "tab", kind: "same_as" },
];
const lit = (...focused: string[]) => [...relationLinks(relations, items, new Set(focused))];

test("a focused card lights only its own ties: evidence pages, named subjects, the person's links", () => {
  expect(lit("finding")).toEqual(["a-finding"]);
  expect(lit("subject")).toEqual(["a-subject", "b-subject"]);
  expect(lit("step")).toEqual(["step-subject"]);
  // A result, the request, Sources and a page light nothing.
  expect(lit("result")).toEqual([]);
  expect(lit("request")).toEqual([]);
  expect(lit("sources")).toEqual([]);
  expect(lit("page-a")).toEqual([]);
  expect(lit("note")).toEqual(["note-tab"]);
});

test("past six ties a card lights none", () => {
  const many = Array.from({ length: 7 }, (_, index): CanvasLink => ({
    id: `page-${index}`,
    source: `page-${index}`,
    target: "subject",
    kind: "supports",
    role: "evidence",
  }));
  expect(relationLinks(many.slice(0, 6), items, new Set(["subject"])).size).toBe(6);
  expect(relationLinks(many, items, new Set(["subject"])).size).toBe(0);
});

test("no relation edge ends at a request: the group edges already read from it", () => {
  const links = environmentLinks({
    ...snapshot,
    elements: [
      ...snapshot.elements,
      {
        id: "notes",
        area: null,
        reference: {
          kind: "artifact",
          objective: "objective",
          execution: "execution",
          artifact: "n",
        },
      },
    ],
    relations: [
      {
        id: "asked",
        from: "objective-card",
        to: "result-card",
        kind: "uses",
        origin: { kind: "user" },
      },
      { id: "cites", from: "notes", to: "result-card", kind: "supports", origin: { kind: "user" } },
    ],
  } as typeof snapshot);
  expect(links.map((link) => [link.source, link.kind, link.target])).toEqual([
    ["notes", "supports", "result-card"],
  ]);
});

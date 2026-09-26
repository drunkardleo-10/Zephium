import { expect, test } from "vitest";
import { relationLinks, restLink, type CanvasItem, type CanvasLink } from "../lib/canvas-model";
import { environmentLinks } from "../lib/project-environment";
import { snapshot } from "./environment-fixtures";

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
    card("note", "note"),
    card("tab", "tab"),
    card("link", "link"),
    card("request", "request"),
    card("block", "block"),
    card("sources", "sources"),
  ].map((item) => [item.id, item]),
);
const relations: CanvasLink[] = [
  { id: "note-tab", source: "note", target: "tab", kind: "same_as" },
  { id: "note-link", source: "note", target: "link", kind: "uses" },
  { id: "block-note", source: "block", target: "note", kind: "supports" },
];
const lit = (...focused: string[]) => [...relationLinks(relations, items, new Set(focused))];

test("a focused card lights only the person's own ties; a board and its column light nothing", () => {
  expect(lit("note")).toEqual(["note-tab", "note-link"]);
  expect(lit("tab")).toEqual(["note-tab"]);
  expect(lit("block")).toEqual([]);
  expect(lit("request")).toEqual([]);
  expect(lit("sources")).toEqual([]);
  expect(restLink({ id: "t", source: "a", target: "b", kind: "thread" })).toBe(true);
  expect(restLink(relations[0]!)).toBe(false);
});

test("past six ties a card lights none", () => {
  const many = Array.from({ length: 7 }, (_, index): CanvasLink => ({
    id: `tie-${index}`,
    source: "note",
    target: index % 2 ? "tab" : "link",
    kind: "uses",
  }));
  expect(relationLinks(many.slice(0, 6), items, new Set(["note"])).size).toBe(6);
  expect(relationLinks(many, items, new Set(["note"])).size).toBe(0);
});

test("no relation edge reaches what a request's board draws", () => {
  const links = environmentLinks({
    ...snapshot,
    elements: [
      ...snapshot.elements,
      { id: "note", area: null, reference: { kind: "resource", resource: "n" } },
      { id: "tab", area: null, reference: { kind: "resource", resource: "t" } },
    ],
    relations: [
      {
        id: "asked",
        from: "objective-card",
        to: "result-card",
        kind: "uses",
        origin: { kind: "user" },
      },
      { id: "cites", from: "note", to: "result-card", kind: "supports", origin: { kind: "user" } },
      { id: "same", from: "note", to: "tab", kind: "same_as", origin: { kind: "user" } },
    ],
  } as typeof snapshot);
  expect(links.map((link) => [link.source, link.kind, link.target])).toEqual([
    ["note", "same_as", "tab"],
  ]);
});

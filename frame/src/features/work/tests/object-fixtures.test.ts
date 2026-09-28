import { describe, expect, test } from "vitest";
import type {
  Detail,
  ObjectActions,
  ObjectKind,
  ObjectView,
  PickView,
  PlanStepView,
  SheetColumn,
  SheetRow,
} from "../lib/board/types";
import { allObjects } from "./object-fixtures";

// The fixtures stand for what the runtime may emit, so they hold to its limits (spec §3).
const within = (text: string | undefined, max: number) => (text ?? "").length <= max;

function pickFaults(item: PickView): string[] {
  const faults: string[] = [];
  if (!within(item.name, 60)) faults.push(`name ${item.name}`);
  if (!within(item.subtitle, 60)) faults.push(`subtitle ${item.name}`);
  if (item.facts.length > 4) faults.push(`facts ${item.name}`);
  for (const fact of item.facts)
    if (!within(fact.label, 18) || !within(fact.value, 32)) faults.push(`fact ${fact.label}`);
  if (!within(item.why, 120)) faults.push(`why ${item.name}`);
  if (item.tags.length > 3 || item.tags.some((tag) => !within(tag, 16)))
    faults.push(`tags ${item.name}`);
  if (item.price && !within(item.price.display, 24)) faults.push(`price ${item.name}`);
  if (item.route && (!within(item.route.from, 40) || !within(item.route.to, 40)))
    faults.push(`route ${item.name}`);
  return faults;
}

const stepFaults = (step: PlanStepView) =>
  within(step.title, 70) &&
  within(step.detail, 140) &&
  within(step.when, 32) &&
  within(step.cost, 24)
    ? []
    : [`step ${step.title}`];

function sheetFaults(columns: readonly SheetColumn[], rows: readonly SheetRow[]): string[] {
  const faults: string[] = [];
  if (columns.length < 1 || columns.length > 10) faults.push("columns");
  for (const row of rows) {
    if (row.cells.length !== columns.length) faults.push(`row ${row.cells[0]}`);
    row.cells.forEach((cell, index) => {
      const column = columns[index];
      if (column?.kind === "yes_no" && !["yes", "no", "partial", "unknown"].includes(cell))
        faults.push(`yes_no ${cell}`);
      if (column?.kind === "rating" && !/^\d+(\.\d+)?\/\d+$/u.test(cell))
        faults.push(`rating ${cell}`);
      if (!within(cell, 60)) faults.push(`cell ${cell}`);
    });
  }
  return faults;
}

function faults(object: ObjectView): string[] {
  switch (object.kind) {
    case "reply":
      return [
        ...(within(object.headline, 80) ? [] : ["headline"]),
        ...(within(object.text, 480) ? [] : ["text"]),
        ...(object.figures.length <= 4 ? [] : ["figures"]),
        ...object.figures.flatMap((figure) =>
          within(figure.label, 24) && within(figure.value, 20) && within(figure.note, 40)
            ? []
            : [`figure ${figure.label}`],
        ),
        ...(object.points.length <= 5 && object.points.every((point) => within(point, 110))
          ? []
          : ["points"]),
      ];
    case "picks":
      return [
        ...(object.items.length >= 1 && object.items.length <= 12 ? [] : ["items"]),
        ...(object.items.filter((item) => item.recommended).length <= 1 ? [] : ["recommended"]),
        ...object.items.flatMap(pickFaults),
      ];
    case "plan":
      return object.steps.length >= 1 && object.steps.length <= 40
        ? object.steps.flatMap(stepFaults)
        : ["steps"];
    case "list":
      return object.items.flatMap((item) =>
        within(item.title, 90) && within(item.detail, 160) && within(item.from?.quote, 200)
          ? []
          : [`item ${item.title}`],
      );
    case "sheet":
      return sheetFaults(object.columns, object.rows);
    case "diagram":
      return [
        ...(object.diagram.nodes.length <= 24 ? [] : ["nodes"]),
        ...object.diagram.nodes.flatMap((node) =>
          within(node.name, 28) && within(node.note, 60) ? [] : [`node ${node.name}`],
        ),
        ...object.diagram.edges.flatMap((edge) =>
          within(edge.label, 24) ? [] : [`edge ${edge.label}`],
        ),
      ];
    case "diff":
      return within(object.summary, 120) ? [] : ["summary"];
    case "draft":
      return within(object.body, 4000) && within(object.subject, 120) ? [] : ["draft"];
    case "media":
      return within(object.title, 80) ? [] : ["title"];
    default:
      return [];
  }
}

describe("object fixtures", () => {
  test.each(allObjects.map((object) => [object.id, object] as const))(
    "%s holds to its limits",
    (_, object) => {
      expect(faults(object)).toEqual([]);
    },
  );

  test("cover every object kind", () => {
    const kinds = {
      reply: 0,
      picks: 0,
      plan: 0,
      list: 0,
      sheet: 0,
      plot: 0,
      diagram: 0,
      code: 0,
      diff: 0,
      document: 0,
      draft: 0,
      media: 0,
      page: 0,
      note: 0,
      file: 0,
      folder: 0,
    } satisfies Record<ObjectKind, number>;
    for (const object of allObjects) kinds[object.kind] += 1;
    expect(Object.entries(kinds).filter(([, count]) => !count)).toEqual([]);
  });

  test("ask nothing of their canvas at three levels", () => {
    const levels = ["full", "overview", "tile"] as const satisfies readonly Detail[];
    const none: ObjectActions = {};
    expect(levels).toHaveLength(3);
    expect(Object.keys(none)).toEqual([]);
  });
});

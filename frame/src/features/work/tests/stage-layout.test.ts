import { expect, test } from "vitest";
import {
  SIZES,
  laneShape,
  laneSlots,
  markStand,
  placeLane,
  stageLayout,
  type StageContents,
  type StageMember,
} from "../lib/stage-layout";

const request = { x: 0, y: 0, ...SIZES.request };
const cards = (prefix: string, count: number, size: { width: number; height: number }) =>
  Array.from({ length: count }, (_, index): StageMember => ({ id: `${prefix}${index}`, size }));
const step = { width: 248, height: 120 };

function full(): StageContents {
  return {
    sources: { members: cards("sources", 1, SIZES.sources) },
    pages: { members: cards("page", 5, SIZES.page) },
    subjects: { members: cards("subject", 4, SIZES.subject) },
    findings: { members: cards("findings", 1, SIZES.findings) },
    results: { members: cards("result", 1, SIZES.document) },
    plan: {
      members: cards("step", 2, step).map((member) => ({ ...member, of: "result0" })),
    },
  };
}

test("a full lane: request, then Worked with, Found and Made in slots 1 to 3", () => {
  const layout = stageLayout(request, full());
  expect(layout.groups.map((group) => [group.kind, group.box])).toEqual([
    // Sources, then pages two across: 24 + 300 + 16 + 248 + 16 + 248 + 24.
    ["worked", { x: 348, y: 0, width: 876, height: 604 }],
    // Subjects three across over the findings: 24 + 3 × 220 + 2 × 16 + 24.
    ["found", { x: 348 + 876 + 48, y: 0, width: 740, height: 572 }],
    // The result, the 48 px gutter, then its steps in their own box.
    ["made", { x: 348 + 876 + 48 + 740 + 48, y: 0, width: 1060, height: 24 + 20 + 300 + 24 }],
  ]);
  const worked = 348;
  const found = 1272;
  const made = 2060;
  expect(layout.positions).toMatchObject({
    // A 24 px pad and a 20 px caption above the first cards.
    sources0: { x: worked + 24, y: 44 },
    page0: { x: worked + 340, y: 44 },
    page1: { x: worked + 604, y: 44 },
    page2: { x: worked + 340, y: 228 },
    page4: { x: worked + 340, y: 412 },
    subject0: { x: found + 24, y: 44 },
    subject2: { x: found + 496, y: 44 },
    subject3: { x: found + 24, y: 196 },
    findings0: { x: found + 24, y: 348 },
    result0: { x: made + 24, y: 44 },
    step0: { x: made + 24 + 420 + 48 + 16, y: 44 + 16 + 20 },
    step1: { x: made + 24 + 420 + 48 + 16 + 248 + 16, y: 80 },
  });
  expect(layout.groups[2]!.steps).toEqual([
    {
      result: "result0",
      box: { x: made + 492, y: 44, width: 2 * 248 + 16 + 32, height: 120 + 32 + 20 },
      members: ["step0", "step1"],
    },
  ]);
  expect(layout.extent).toBe(604);
});

test("a pure result lays out request → Made in slot 1", () => {
  const layout = stageLayout(request, { results: { members: cards("result", 1, SIZES.document) } });
  expect(layout.groups.map((group) => [group.kind, group.box.x])).toEqual([["made", 348]]);
  expect(layout.positions.result0).toEqual({ x: 372, y: 44 });
  // Narrower than a slot, the group keeps its own width; the slot is never under 320.
  expect(laneSlots([laneShape({ results: { members: cards("r", 1, SIZES.file) } })])).toEqual([
    348,
  ]);
});

test("lanes that share a shape align their groups into columns, sized by the widest", () => {
  const first = laneShape(full());
  const second = laneShape({
    sources: { members: cards("sources", 1, SIZES.sources) },
    pages: { members: cards("page", 1, SIZES.page) },
    subjects: { members: cards("subject", 6, SIZES.subject) },
    results: { members: cards("result", 1, SIZES.comparison) },
  });
  const slots = laneSlots([first, second]);
  const top = placeLane(request, first, slots);
  const below = placeLane({ ...request, y: top.extent + 96 }, second, slots);
  expect(below.request.y).toBe(604 + 96);
  expect(below.groups.map((group) => group.box.x)).toEqual(top.groups.map((group) => group.box.x));
  // The first lane's groups are the widest in every slot.
  expect(slots).toEqual([348, 1272, 2060]);
});

test("a compacted lane never widens another lane's slot, and never overlaps itself", () => {
  const research = laneShape(full());
  const pure = laneShape({
    results: { members: cards("result", 1, SIZES.document) },
    plan: { members: cards("step", 4, step).map((member) => ({ ...member, of: "result0" })) },
  });
  expect(laneSlots([research, pure])).toEqual(laneSlots([research]));
  const only = laneShape({
    subjects: { members: cards("subject", 6, SIZES.subject) },
    results: { members: cards("result", 1, SIZES.comparison) },
  });
  const layout = placeLane(request, only, laneSlots([research, only]));
  const [found, made] = layout.groups;
  expect(made!.box.x).toBeGreaterThanOrEqual(found!.box.x + found!.box.width + 48);
});

test("a lane that only worked locally has one row of files and commands", () => {
  const layout = stageLayout(request, {
    work: { members: [...cards("file", 3, SIZES.file), ...cards("command", 1, SIZES.command)] },
  });
  const [worked] = layout.groups;
  expect(worked!.kind).toBe("worked");
  expect(worked!.local).toEqual({ x: 372, y: 44, width: 2 * 248 + 16, height: 96 + 16 + 120 });
  expect([layout.positions.file0, layout.positions.file1, layout.positions.file2]).toEqual([
    { x: 372, y: 44 },
    { x: 372 + 264, y: 44 },
    { x: 372, y: 156 },
  ]);
  expect(layout.positions.command0).toEqual({ x: 636, y: 156 });
});

test("the local row sits under the web row inside the same group", () => {
  const layout = stageLayout(request, {
    sources: { members: cards("sources", 1, SIZES.sources) },
    work: { members: cards("file", 1, SIZES.file) },
  });
  expect(layout.groups).toHaveLength(1);
  expect(layout.positions.file0).toEqual({ x: 372, y: 44 + 200 + 16 });
});

test("past eight pages or twelve subjects the rest only count on the caption", () => {
  const layout = stageLayout(request, {
    pages: { members: cards("page", 11, SIZES.page) },
    subjects: { members: cards("subject", 12, SIZES.subject), more: 5 },
  });
  const [worked, found] = layout.groups;
  expect([worked!.members.length, worked!.more, worked!.counts.pages]).toEqual([8, 3, 11]);
  expect([found!.members.length, found!.more, found!.counts.subjects]).toEqual([12, 5, 17]);
  expect(layout.positions.page8).toBeUndefined();
});

test("the mark stands by what the agent acts on", () => {
  const contents = full();
  const layout = stageLayout(request, contents);
  const sizes = Object.fromEntries(
    Object.values(contents).flatMap((group) =>
      group!.members.map((member) => [member.id, member.size] as const),
    ),
  );
  expect(markStand(layout, { doing: "thinking" })).toEqual({ x: 316, y: 0 });
  // The orb is centred on Worked with's top-left corner.
  expect(markStand(layout, { doing: "searching" })).toEqual({ x: 336, y: -12 });
  // Eight pixels outside the top-right corner of the page being read.
  expect(markStand(layout, { doing: "reading", page: "page1" }, sizes)).toEqual({
    x: 348 + 604 + 248 + 8,
    y: 44 - 8 - 24,
  });
  expect(markStand(layout, { doing: "writing" })).toEqual({ x: 2060 - 12, y: -12 });
  expect(markStand(layout, { doing: "done", result: "result0" }, sizes)).toEqual({
    x: 2060 + 24 + 420 + 8,
    y: 12,
  });
  // Before a group exists the mark waits where it will stand.
  const empty = stageLayout(request, {});
  expect(markStand(empty, { doing: "searching" }, {}, [348])).toEqual({ x: 336, y: -12 });
  const local = stageLayout(request, { work: { members: cards("file", 1, SIZES.file) } });
  expect(markStand(local, { doing: "working" })).toEqual({ x: 372 + 248 + 8, y: 44 });
});

test("Made reads as a set: the cover and its steps, the diagram and its area, then two across", () => {
  const part = { width: 180, height: 56 };
  const layout = stageLayout(request, {
    results: {
      members: [
        { id: "cover", size: SIZES.document, cover: true },
        { id: "diagram", size: { width: 300, height: 110 } },
        { id: "table0", size: { width: 360, height: 200 } },
        { id: "table1", size: { width: 420, height: 240 } },
        { id: "chart", size: { width: 324, height: 250 } },
      ],
    },
    plan: { members: cards("step", 2, step).map((member) => ({ ...member, of: "cover" })) },
    diagram: {
      members: [
        { id: "a", size: part, of: "diagram", at: { x: 0, y: 0 } },
        { id: "b", size: part, of: "diagram", at: { x: 212, y: 0 } },
        { id: "c", size: part, of: "diagram", at: { x: 212, y: 68 } },
      ],
    },
  });
  const made = layout.groups[0]!;
  const x = 348;
  expect(made.box).toEqual({ x, y: 0, width: 1060, height: 1082 });
  expect(layout.positions).toMatchObject({
    cover: { x: x + 24, y: 44 },
    step0: { x: x + 508, y: 80 },
    step1: { x: x + 772, y: 80 },
    // The diagram on its own row, its area 16 px to the right of its cover.
    diagram: { x: x + 24, y: 360 },
    a: { x: x + 356, y: 396 },
    b: { x: x + 568, y: 396 },
    c: { x: x + 568, y: 464 },
    // Tables and the chart two across under the rows.
    table0: { x: x + 24, y: 552 },
    table1: { x: x + 400, y: 552 },
    chart: { x: x + 24, y: 808 },
  });
  expect(made.steps?.map((entry) => entry.box)).toEqual([
    { x: x + 492, y: 44, width: 544, height: 172 },
  ]);
  expect(made.diagrams).toEqual([
    {
      result: "diagram",
      box: { x: x + 340, y: 360, width: 424, height: 176 },
      members: ["a", "b", "c"],
    },
  ]);
  expect(made.counts).toEqual({ results: 5, plan: 2 });
});

test("without a diagram, the cover's row is followed by the rest two across", () => {
  const layout = stageLayout(request, {
    results: {
      members: [
        { id: "cover", size: SIZES.document, cover: true },
        { id: "table", size: { width: 360, height: 200 } },
        { id: "chart", size: { width: 324, height: 250 } },
      ],
    },
  });
  const made = layout.groups[0]!;
  expect(layout.positions).toMatchObject({
    cover: { x: 348 + 24, y: 44 },
    table: { x: 348 + 24, y: 360 },
    chart: { x: 348 + 400, y: 360 },
  });
  expect(made.box).toEqual({ x: 348, y: 0, width: 748, height: 634 });
  expect(made.diagrams).toBeUndefined();
});

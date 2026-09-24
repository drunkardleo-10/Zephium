import { expect, test } from "vitest";
import { SIZES, STAGE_GAP, stageLayout, stageStand, type StageMember } from "../lib/stage-layout";

const request = { x: 0, y: 0, ...SIZES.request };
const cards = (prefix: string, count: number, size: { width: number; height: number }) =>
  Array.from({ length: count }, (_, index): StageMember => ({ id: `${prefix}${index}`, size }));

test("a stage reads left to right in grids, and the next request clears its tallest cluster", () => {
  const layout = stageLayout(request, {
    sources: { members: cards("sources", 1, SIZES.sources) },
    pages: { members: cards("page", 5, SIZES.page) },
    subjects: { members: cards("subject", 4, SIZES.subject) },
    findings: { members: cards("findings", 1, SIZES.findings) },
    results: { members: cards("result", 1, SIZES.document) },
  });
  const sources = 300 + 48;
  const pages = sources + 300 + 48;
  const subjects = pages + 2 * 248 + 20 + 48;
  const findings = subjects + 3 * 220 + 2 * 20 + 48;
  const result = findings + 300 + 48;
  expect(layout.positions).toEqual({
    sources0: { x: sources, y: 0 },
    page0: { x: pages, y: 0 },
    page1: { x: pages + 268, y: 0 },
    page2: { x: pages, y: 188 },
    page3: { x: pages + 268, y: 188 },
    page4: { x: pages, y: 376 },
    subject0: { x: subjects, y: 0 },
    subject1: { x: subjects + 240, y: 0 },
    subject2: { x: subjects + 480, y: 0 },
    subject3: { x: subjects, y: 156 },
    findings0: { x: findings, y: 0 },
    result0: { x: result, y: 0 },
  });
  expect(layout.clusters.map((cluster) => [cluster.kind, cluster.members.length])).toEqual([
    ["sources", 1],
    ["pages", 5],
    ["subjects", 4],
    ["findings", 1],
    ["results", 1],
  ]);
  // Three rows of pages are the tallest cluster.
  expect(layout.extent).toBe(3 * 168 + 2 * 20);
  expect(
    stageStand(layout, { request, at: ["results"], size: { width: 260, height: 84 } }),
  ).toEqual({ x: result + 420 + 16, y: 0 });

  const next = { x: 0, y: layout.extent + STAGE_GAP, ...SIZES.request };
  const second = stageLayout(next, { pages: { members: cards("later", 1, SIZES.page) } });
  expect(second.positions.later0).toEqual({ x: 300 + 48, y: 544 + 56 });
});

test("past eight pages or twelve subjects the rest only count on the label", () => {
  const layout = stageLayout(request, {
    pages: { members: cards("page", 11, SIZES.page) },
    subjects: { members: cards("subject", 12, SIZES.subject), more: 5 },
  });
  const [pages, subjects] = layout.clusters;
  expect([pages!.members.length, pages!.more]).toEqual([8, 3]);
  expect([subjects!.members.length, subjects!.more]).toEqual([12, 5]);
  expect(layout.positions.page8).toBeUndefined();
  expect(Object.keys(layout.positions)).toHaveLength(20);
});

test("saved cards keep their place, and a cluster narrows rather than grow into one", () => {
  const layout = stageLayout(request, {
    pages: { members: cards("page", 3, SIZES.page) },
    subjects: {
      members: [
        { id: "kept", size: SIZES.subject, placed: { x: 700, y: 0 } },
        { id: "new", size: SIZES.subject },
      ],
    },
  });
  // Two pages across would reach 864 and run into the subjects at 700.
  expect([layout.positions.page0, layout.positions.page1, layout.positions.page2]).toEqual([
    { x: 348, y: 0 },
    { x: 348, y: 188 },
    { x: 348, y: 376 },
  ]);
  expect(layout.positions.kept).toEqual({ x: 700, y: 0 });
  expect(layout.positions.new).toEqual({ x: 940, y: 0 });
});

test("the agent's stand steps down past a cluster it would cover, never onto a card", () => {
  const layout = stageLayout(request, {
    sources: { members: cards("sources", 1, SIZES.sources) },
    pages: { members: cards("page", 2, SIZES.page) },
  });
  const size = { width: 260, height: 104 };
  // Right of Sources the capsule would sit on the pages; one row of them is shorter to clear.
  const stand = stageStand(layout, { request, at: ["sources"], size });
  expect(stand).toEqual({ x: 348 + 300 + 16, y: 168 + 16 });
  for (const box of [request, ...layout.clusters.map((cluster) => cluster.box)])
    expect(
      stand.x < box.x + box.width &&
        box.x < stand.x + size.width &&
        stand.y < box.y + box.height &&
        box.y < stand.y + size.height,
    ).toBe(false);
});

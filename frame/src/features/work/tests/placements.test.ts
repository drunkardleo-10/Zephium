import { expect, test } from "vitest";
import type { WorkEnvironmentSnapshot } from "$shared/ipc/bindings";
import { reconcileNodes, type CanvasItem } from "../lib/canvas-model";
import { environmentView, viewPlacements } from "../lib/project-environment";
import { environmentRequests } from "../lib/project-environment-thread";
import { environmentStages, requestTextSize } from "../lib/project-environment-board";
import { projection, snapshot } from "./environment-fixtures";

type Placement = WorkEnvironmentSnapshot["view"]["placements"][number];
function canvas(placements: Placement[]): WorkEnvironmentSnapshot {
  return { ...snapshot, view: { ...snapshot.view, placements } };
}
const objectives = () => new Map([["objective", structuredClone(projection)]]);
test("a placement saved before boards is ignored: the block takes its board place", () => {
  const legacy = canvas([
    { element: "objective-card", x: 80, y: 120, width: 300, height: 110 },
    { element: "result-card", x: 30, y: -20, width: 480, height: 360, revision: 2 },
  ]);
  const stages = environmentStages(legacy, objectives());
  expect(environmentRequests(stages).positions).toMatchObject({
    "objective-card": { x: 0, y: 0 },
    "result-card": stages[0]!.targets["result-card"],
  });
  // Without parts the answer stands a fork's width past the words, one line joining them.
  const end = stages[0]!.lane.requestEnd;
  expect(end).toBeLessThan(320);
  expect(stages[0]!.targets["result-card"]!.x).toBe(Math.ceil((end + 96) / 8) * 8);
  const line = stages[0]!.lane.lines.find((entry) => entry.kind === "part")!;
  expect(line.points[0]).toMatchObject({ x: end + 6 });
  const view = environmentView(legacy);
  expect(view.positions).toEqual({});
  expect(view.sizes).toEqual({});
});

test("a request is as tall as its words at rest, whatever size an older placement saved", () => {
  const saved = canvas([
    { element: "objective-card", x: 0, y: 0, width: 300, height: 110, revision: 2 },
  ]);
  const state = structuredClone(projection);
  state.executions[0]!.spec.request =
    "Compare hosting for a small SaaS: prices, regions, managed Postgres, backups, bandwidth " +
    "and support of AWS Lightsail, Hetzner, Hostinger and Vercel, then pick one.";
  const stages = environmentStages(saved, new Map([["objective", state]]));
  // Two lines at rest: the person opens the rest where it stands.
  const words = requestTextSize(stages[0]!.request, false);
  expect(words.height).toBeGreaterThan(80);
  expect(stages[0]!.place).toMatchObject(words);
  expect(environmentView(saved).sizes).toEqual({});
});

test("a card follows its lane place when that place moves, and only then", () => {
  const item: CanvasItem = { id: "card", title: "Card", kind: "", detail: "", status: "" };
  const first = reconcileNodes([], [item], { card: { x: 100, y: 0 } });
  // The person drags it; the lane has not moved, so the card stays where they put it.
  const dragged = [{ ...first[0]!, position: { x: 140, y: 30 } }];
  const still = reconcileNodes(
    dragged,
    [item],
    { card: { x: 100, y: 0 } },
    {},
    [],
    {},
    {
      card: { x: 100, y: 0 },
    },
  );
  expect(still[0]!.position).toEqual({ x: 140, y: 30 });
  // The lane re-lays out: the card takes its new place.
  const followed = reconcileNodes(
    still,
    [item],
    { card: { x: 400, y: 0 } },
    {},
    [],
    {},
    {
      card: { x: 100, y: 0 },
    },
  );
  expect(followed[0]!.position).toEqual({ x: 400, y: 0 });
});

test("a saved placement always holds to the checkpoint contract", () => {
  const stages = environmentStages(snapshot, objectives());
  const saved = viewPlacements(
    snapshot,
    {
      positions: {
        "objective-card": { x: 0.4, y: 0 },
        "result-card": { x: 2_500_000, y: Number.NaN },
      },
      sizes: {
        "objective-card": { width: 300, height: 64 },
        "result-card": { width: 90.6, height: 9000 },
      },
      viewport: { x: 0, y: 0, zoom: 1 },
    },
    stages,
  );
  for (const place of saved) {
    for (const value of [place.x, place.y, place.width, place.height])
      expect(Number.isInteger(value)).toBe(true);
    expect(Math.abs(place.x)).toBeLessThanOrEqual(1_000_000);
    expect(Math.abs(place.y)).toBeLessThanOrEqual(1_000_000);
  }
  // A one-line request measures 64; the contract's floor is 80.
  expect(saved.find((place) => place.element === "objective-card")).toMatchObject({
    width: 300,
    height: 80,
  });
  expect(saved.find((place) => place.element === "result-card")).toMatchObject({
    width: 120,
    height: 4096,
    y: 0,
  });
});

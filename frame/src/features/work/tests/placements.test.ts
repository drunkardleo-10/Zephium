import { expect, test } from "vitest";
import type { WorkEnvironmentSnapshot, WorkRuntimeProjection } from "$shared/ipc/bindings";
import { reconcileNodes, type CanvasItem } from "../lib/canvas-model";
import { requestSize } from "../lib/card-size";
import { environmentView, viewPlacements } from "../lib/project-environment";
import { environmentRequests, environmentStages } from "../lib/project-environment-thread";
import { projection, snapshot } from "./environment-fixtures";

type Placement = WorkEnvironmentSnapshot["view"]["placements"][number];
function canvas(placements: Placement[]): WorkEnvironmentSnapshot {
  return { ...snapshot, view: { ...snapshot.view, placements } };
}
const objectives = () => new Map([["objective", structuredClone(projection)]]);
/** The same work once it also read a page: Worked with takes slot 1 and Made moves right. */
function withPage(): Map<string, WorkRuntimeProjection> {
  const state = structuredClone(projection);
  const run = state.executions[0]!;
  run.spec.nodes[0]!.capability = {
    kind: "agent",
    grant: {
      provider: "open_ai",
      model: "gpt-5.6-luna",
      max_turns: 10,
      max_steps: 32,
      browse_hops: 4,
    },
  };
  run.steps = [
    {
      id: "read",
      turn: 1,
      kind: { kind: "read", url: "https://a.example/one" },
      status: "succeeded",
    },
  ];
  return new Map([["objective", state]]);
}

test("a placement saved before lanes is ignored once: the card takes its lane place", () => {
  const legacy = canvas([
    { element: "objective-card", x: 80, y: 120, width: 300, height: 110 },
    { element: "result-card", x: 900, y: 700, width: 480, height: 360 },
  ]);
  const stages = environmentStages(legacy, objectives());
  expect(environmentRequests(stages).positions).toMatchObject({
    "objective-card": { x: 0, y: 0 },
    "result-card": { x: 372, y: 44 },
  });
  const view = environmentView(legacy);
  expect(view.positions).toEqual({});
  expect(view.sizes).toEqual({});
  // Saved again, it is written in lane terms: no offset, revision 2.
  const saved = viewPlacements(
    legacy,
    {
      positions: { "objective-card": { x: 0, y: 0 }, "result-card": { x: 372, y: 44 } },
      sizes: { "result-card": { width: 420, height: 300 } },
      viewport: { x: 0, y: 0, zoom: 1 },
    },
    stages,
  );
  expect(saved.find((place) => place.element === "result-card")).toEqual({
    element: "result-card",
    x: 0,
    y: 0,
    width: 420,
    height: 300,
    revision: 2,
  });
});

test("a card the person moved keeps its offset when its lane lays out again", () => {
  const moved = canvas([
    { element: "result-card", x: 30, y: -20, width: 420, height: 160, revision: 2 },
  ]);
  const before = environmentStages(moved, objectives());
  expect(environmentRequests(before).positions["result-card"]).toEqual({ x: 372 + 30, y: 24 });
  // The run reads a page: Made moves to slot 2, and the card moves with it.
  const after = environmentStages(moved, withPage());
  const lane = after[0]!.layout.positions["result-card"]!;
  expect(lane.x).toBeGreaterThan(372);
  expect(environmentRequests(after).positions["result-card"]).toEqual({
    x: lane.x + 30,
    y: lane.y - 20,
  });
  // Saving the card where it stands writes the same offset back.
  const saved = viewPlacements(
    moved,
    {
      positions: { "result-card": { x: lane.x + 30, y: lane.y - 20 } },
      viewport: { x: 0, y: 0, zoom: 1 },
    },
    after,
  );
  expect(saved.find((place) => place.element === "result-card")).toMatchObject({
    x: 30,
    y: -20,
    revision: 2,
  });
});

test("a saved size still applies, to the card and to its lane", () => {
  const resized = canvas([
    { element: "result-card", x: 0, y: 0, width: 640, height: 480, revision: 2 },
  ]);
  const stages = environmentStages(resized, objectives());
  expect(stages[0]!.contents.results!.members[0]!.size).toEqual({ width: 640, height: 480 });
  expect(stages[0]!.layout.groups[0]!.box.width).toBe(24 + 640 + 24);
  expect(environmentView(resized).sizes).toEqual({ "result-card": { width: 640, height: 480 } });
});

test("a request card is as tall as its words, whatever size an older placement saved", () => {
  const saved = canvas([
    { element: "objective-card", x: 0, y: 0, width: 300, height: 110, revision: 2 },
  ]);
  const state = structuredClone(projection);
  state.executions[0]!.spec.request =
    "Compare hosting for a small SaaS: prices, regions, managed Postgres, backups, bandwidth " +
    "and support of AWS Lightsail, Hetzner, Hostinger and Vercel, then pick one.";
  const stages = environmentStages(saved, new Map([["objective", state]]));
  const words = requestSize(stages[0]!.request, { footer: !!stages[0]!.facts.counts });
  expect(words.height).toBeGreaterThan(110);
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

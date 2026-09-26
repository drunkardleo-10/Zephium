import { expect, test } from "vitest";
import { environmentRequests, environmentStages } from "../lib/project-environment-thread";
import { environmentItems } from "../lib/project-environment";
import { projection, snapshot } from "./environment-fixtures";
import type { WorkEnvironmentSnapshot, WorkRuntimeProjection } from "$shared/ipc/bindings";

/** Three messages on one work: the first one, then two continuations. */
function thread(): {
  snapshot: WorkEnvironmentSnapshot;
  objectives: Map<string, WorkRuntimeProjection>;
} {
  const state = structuredClone(projection);
  const first = state.executions[0]!;
  first.spec.request = "Compare quiet keyboards";
  state.executions = ["Show me the quietest one", "And the wireless ones"].reduce(
    (executions, request, index) => {
      const next = structuredClone(first);
      next.id = `continuation-${index + 1}`;
      next.spec.request = request;
      return [...executions, next];
    },
    [first],
  );
  state.work.objective = "And the wireless ones";
  return {
    snapshot: {
      ...snapshot,
      view: {
        ...snapshot.view,
        placements: [
          { element: "objective-card", x: 0, y: 0, width: 320, height: 150 },
          { element: "result-card", x: 800, y: 300, width: 420, height: 360 },
        ],
      },
    },
    objectives: new Map([["objective", state]]),
  };
}

test("every message keeps its own request card, one lane each down the request column", () => {
  const { snapshot: scene, objectives } = thread();
  const stages = environmentStages(scene, objectives);
  // The first lane holds the result in Made; a saved absolute place from before lanes is ignored.
  const made = stages[0]!.layout.groups[0]!;
  expect([made.kind, made.box.x, made.box.y]).toEqual(["made", 348, 0]);
  const first = made.box.height;
  expect(stages.map((stage) => [stage.card, stage.request, stage.place.x, stage.place.y])).toEqual([
    ["objective-card", "Compare quiet keyboards", 0, 0],
    ["request:objective-card:continuation-1", "Show me the quietest one", 0, first + 96],
    // A lane with nothing but its request is as tall as the request: 80 px, its floor.
    ["request:objective-card:continuation-2", "And the wireless ones", 0, first + 96 + 80 + 96],
  ]);
  // The request card the work began with keeps the first sentence, not the last.
  expect(environmentItems(scene, [], [], objectives)[0]?.title).toBe("Compare quiet keyboards");
  const requests = environmentRequests(stages);
  expect(requests.items.map((item) => [item.id, item.type, item.title])).toEqual([
    ["request:objective-card:continuation-1", "request", "Show me the quietest one"],
    ["request:objective-card:continuation-2", "request", "And the wireless ones"],
  ]);
  // The thread joins request to request; the lane between them reads to its result.
  expect(requests.links.map((link) => [link.kind, link.source, link.target])).toEqual([
    ["thread", "objective-card", "request:objective-card:continuation-1"],
    ["thread", "request:objective-card:continuation-1", "request:objective-card:continuation-2"],
  ]);
  expect(requests.positions).toMatchObject({
    "objective-card": { x: 0, y: 0 },
    "result-card": { x: 372, y: 44 },
    "request:objective-card:continuation-1": { x: 0, y: first + 96 },
  });
});

test("the next request clears the tallest group of the lane above it", () => {
  const { snapshot: scene, objectives } = thread();
  const state = objectives.get("objective")!;
  const first = state.executions[0]!;
  first.spec.nodes[0]!.capability = {
    kind: "agent",
    grant: {
      provider: "open_ai",
      model: "gpt-5.6-luna",
      max_turns: 10,
      max_steps: 32,
      browse_hops: 4,
    },
  };
  // Four pages two across: two rows in Worked with, taller than the result beside it.
  first.steps = Array.from({ length: 4 }, (_, index) => ({
    id: `read-${index}`,
    turn: 1,
    kind: { kind: "read" as const, url: `https://a.example/${index}` },
    status: "succeeded" as const,
  }));
  const stages = environmentStages(scene, objectives);
  expect(stages[0]!.layout.groups.map((group) => group.kind)).toEqual(["worked", "made"]);
  expect(stages[1]!.place.y).toBe(24 + 20 + 2 * 168 + 16 + 24 + 96);
  expect(stages[1]!.place.x).toBe(0);
});

test("running the same sentence again continues the stage it began", () => {
  const { snapshot: scene, objectives } = thread();
  const state = objectives.get("objective")!;
  state.executions = state.executions.slice(0, 1);
  const again = structuredClone(state.executions[0]!);
  again.id = "retry";
  state.executions = [state.executions[0]!, again];
  const stages = environmentStages(scene, objectives);
  expect(stages).toHaveLength(1);
  expect(stages[0]!.executions).toEqual(["execution", "retry"]);
});

test("a request says how long its lane took and what it read; a page is named by its title", async () => {
  const { environmentPages } = await import("../lib/project-environment");
  const { snapshot: scene, objectives } = thread();
  const state = objectives.get("objective")!;
  const later = state.executions[1]!;
  later.spec.nodes[0]!.capability = {
    kind: "agent",
    grant: {
      provider: "open_ai",
      model: "gpt-5.6-luna",
      max_turns: 10,
      max_steps: 32,
      browse_hops: 4,
    },
  };
  const measured = (wall_millis: number) =>
    ({
      wall_millis,
      decision_calls: 0,
      emulation_calls: 0,
      planner_calls: 0,
      native_actions: 0,
      model_tokens: 0,
      cost_micro_usd: 0,
    }) as never;
  later.steps = [
    {
      id: "read",
      turn: 1,
      kind: { kind: "read", url: "https://www.airbnb.com/rooms/1" },
      status: "succeeded",
      measurements: measured(3200),
      local: { page_title: "Charming Cole Valley Victorian" },
    },
  ];
  const stages = environmentStages(scene, objectives);
  const [request] = environmentRequests(stages).items;
  expect([request!.elapsed, request!.counts]).toEqual([3200, { sources: 0, pages: 1 }]);
  // The first lane read nothing: its card says so by saying nothing.
  expect(environmentItems(scene, [], [], objectives)[0]!.counts).toBeUndefined();
  const pages = environmentPages(scene, objectives, stages, () => []);
  expect(pages.items.map((item) => [item.title, item.page?.host])).toEqual([
    ["Charming Cole Valley Victorian", "www.airbnb.com"],
  ]);
});

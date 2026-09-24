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

test("every message keeps its own request card, laid out down the roadmap", () => {
  const { snapshot: scene, objectives } = thread();
  const stages = environmentStages(scene, objectives);
  expect(stages.map((stage) => [stage.card, stage.request, stage.place.y])).toEqual([
    ["objective-card", "Compare quiet keyboards", 0],
    ["request:objective-card:continuation-1", "Show me the quietest one", 716],
    ["request:objective-card:continuation-2", "And the wireless ones", 882],
  ]);
  // The request card the work began with keeps the first sentence, not the last.
  expect(environmentItems(scene, [], [], objectives)[0]?.title).toBe("Compare quiet keyboards");
  const requests = environmentRequests(stages);
  expect(requests.items.map((item) => [item.id, item.type, item.title])).toEqual([
    ["request:objective-card:continuation-1", "request", "Show me the quietest one"],
    ["request:objective-card:continuation-2", "request", "And the wireless ones"],
  ]);
  // The thread joins request to request; the stage between them reads to its result.
  expect(requests.links.map((link) => [link.kind, link.source, link.target])).toEqual([
    ["thread", "objective-card", "request:objective-card:continuation-1"],
    ["thread", "request:objective-card:continuation-1", "request:objective-card:continuation-2"],
  ]);
  expect(requests.positions["request:objective-card:continuation-1"]).toEqual({ x: 0, y: 716 });
});

test("the next request clears the tallest column of the stage above it", () => {
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
  // Four pages would sit two across, but the result already stands to their
  // right: the cluster narrows to one column and reaches further than the result.
  first.steps = Array.from({ length: 4 }, (_, index) => ({
    id: `read-${index}`,
    turn: 1,
    kind: { kind: "read" as const, url: `https://a.example/${index}` },
    status: "succeeded" as const,
  }));
  const stages = environmentStages(scene, objectives);
  expect(stages[1]!.place.y).toBe(4 * 168 + 3 * 20 + 56);
  expect(stages[1]!.place.x).toBe(stages[0]!.place.x);
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

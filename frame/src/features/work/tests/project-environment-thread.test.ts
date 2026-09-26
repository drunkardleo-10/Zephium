import { expect, test } from "vitest";
import { environmentRequests } from "../lib/project-environment-thread";
import { environmentStages } from "../lib/project-environment-board";
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
  // The first lane's board holds the result beside the column; a saved absolute place is ignored.
  const first = stages[0]!;
  expect(first.board.blocks.map((block) => [block.id, block.kind])).toEqual([
    ["result-card", "document"],
  ]);
  expect(first.lane.rects["result-card"]).toMatchObject({ x: 348 });
  const below = first.lane.extent + 96;
  expect(stages.map((stage) => [stage.card, stage.request, stage.place.x, stage.place.y])).toEqual([
    ["objective-card", "Compare quiet keyboards", 0, 0],
    ["request:objective-card:continuation-1", "Show me the quietest one", 0, below],
    // A lane with nothing but its request is as tall as the request: 80 px, its floor.
    ["request:objective-card:continuation-2", "And the wireless ones", 0, below + 80 + 96],
  ]);
  // The request card the work began with keeps the first sentence, not the last.
  expect(environmentItems(scene, [], [], objectives)[0]?.title).toBe("Compare quiet keyboards");
  const requests = environmentRequests(stages);
  expect(requests.items.map((item) => [item.id, item.type, item.title])).toEqual([
    ["request:objective-card:continuation-1", "request", "Show me the quietest one"],
    ["request:objective-card:continuation-2", "request", "And the wireless ones"],
  ]);
  // The thread joins request to request down the column.
  expect(requests.links.map((link) => [link.kind, link.source, link.target])).toEqual([
    ["thread", "objective-card", "request:objective-card:continuation-1"],
    ["thread", "request:objective-card:continuation-1", "request:objective-card:continuation-2"],
  ]);
  expect(requests.positions).toMatchObject({
    "objective-card": { x: 0, y: 0 },
    "request:objective-card:continuation-1": { x: 0, y: below },
  });
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

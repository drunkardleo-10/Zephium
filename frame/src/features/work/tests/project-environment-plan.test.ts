import { expect, test } from "vitest";
import type { WorkPlanRevision } from "$shared/ipc/bindings";
import { projection, snapshot } from "./environment-fixtures";
import { environmentItems } from "../lib/project-environment";
import { environmentPlan } from "../lib/project-environment-plan";
import { reconcileNodes, validScene } from "../lib/canvas-model";
const plan: WorkPlanRevision = {
  author: "user",
  revision: "2",
  basis_revision: "1",
  draft: {
    id: "plan",
    nodes: [
      { id: "read", objective: "Read evidence", dependencies: [], outputs: [] },
      { id: "compare", objective: "Compare findings", dependencies: ["read"], outputs: [] },
    ],
  },
};
const states = new Map([["objective", projection]]);
const base = environmentItems(snapshot, [], [], states);
test("expands only the exact historical plan with stable dependency identities and user geometry", () => {
  const scene = environmentPlan(
    snapshot,
    base,
    states,
    new Map([["objective", plan]]),
    "objective-card",
  );
  expect(validScene(scene.items, scene.links)).toBe(true);
  expect(scene.items).toHaveLength(4);
  expect(scene.links).toContainEqual(
    expect.objectContaining({
      source: "plan:objective-card:2:read",
      target: "plan:objective-card:2:compare",
      kind: "dependency",
    }),
  );
  expect(scene.targets.get("plan:objective-card:2:read")).toEqual({
    objective: "objective",
    execution: "execution",
  });
  const nodes = reconcileNodes([], scene.items, scene.positions);
  const moved = nodes.map((node) =>
    node.id === "plan:objective-card:2:read" ? { ...node, position: { x: 17, y: 25 } } : node,
  );
  expect(
    reconcileNodes(moved, scene.items, scene.positions).find(
      (node) => node.id === "plan:objective-card:2:read",
    )?.position,
  ).toEqual({ x: 17, y: 25 });
  const wrong = environmentPlan(
    snapshot,
    base,
    states,
    new Map([["objective", { ...plan, revision: "3" }]]),
    "objective-card",
  );
  expect(wrong.targets.size).toBe(0);
  const collapsed = environmentPlan(snapshot, base, states, new Map([["objective", plan]]), null);
  expect(collapsed.items).toHaveLength(2);
  expect(collapsed.items[0]?.actionLabel).toBe("Show plan");
});

test("large environments keep plan groups compact instead of exceeding the scene cap", () => {
  const full = Array.from({ length: 500 }, (_, i) => ({
    ...base[0]!,
    id: i === 0 ? "objective-card" : `resource-${i}`,
  }));
  const scene = environmentPlan(
    snapshot,
    full,
    states,
    new Map([["objective", plan]]),
    "objective-card",
  );
  expect(scene.items).toHaveLength(500);
  expect(scene.targets.size).toBe(0);
  expect(scene.items[0]?.detail).toContain("Open the objective");
});

test("a newer current draft is never painted with an older terminal execution's status or actors", () => {
  const current = { ...plan, revision: "3" };
  const updated = { ...projection, work: { ...projection.work, plan: current } };
  const scene = environmentPlan(
    snapshot,
    base,
    new Map([["objective", updated]]),
    new Map([["objective", plan]]),
    "objective-card",
  );
  expect(scene.items[0]?.detail).toContain("Current plan · revision 3");
  expect(scene.targets.get("plan:objective-card:3:read")).toEqual({
    objective: "objective",
    execution: null,
  });
  expect(scene.items.find((item) => item.id === "plan:objective-card:3:read")?.status).toBe(
    "Not started",
  );
});

test("responsibilities expose planned output names without duplicating descriptions or inventing artifacts", () => {
  const structured = structuredClone(plan);
  structured.draft.nodes[0]!.outputs = [
    {
      name: "Evidence shortlist",
      description: "Long operational details remain in explicit plan inspection",
      review: "user_acceptance",
    },
  ];
  const scene = environmentPlan(
    snapshot,
    base,
    states,
    new Map([["objective", structured]]),
    "objective-card",
  );
  const item = scene.items.find((item) => item.id === "plan:objective-card:2:read")!;
  expect(item.responsibility).toEqual({ outputs: ["Evidence shortlist"] });
  expect(item.detail).toBe("");
  expect(item.artifact).toBeUndefined();
  const changed = { ...item, responsibility: { outputs: ["Updated shortlist"] } };
  expect(reconcileNodes(reconcileNodes([], [item]), [changed])[0]?.data.responsibility).toEqual(
    changed.responsibility,
  );
});

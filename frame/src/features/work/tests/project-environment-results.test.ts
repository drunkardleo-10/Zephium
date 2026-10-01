import { expect, test } from "vitest";
import { projection, snapshot as savedSnapshot } from "./environment-fixtures";
import { environmentResults } from "../lib/project-environment-results";
import { reconcileNodes } from "../lib/canvas-model";
const snapshot = {
  ...savedSnapshot,
  elements: savedSnapshot.elements.filter((element) => element.reference.kind === "objective"),
};
test("selected objective results use exact facts without creating durable elements", () => {
  const scene = environmentResults(snapshot, [], projection);
  expect(scene.items).toHaveLength(1);
  const item = scene.items[0]!;
  expect(item.artifact?.content).toEqual({ kind: "document", paragraphs: ["Reviewed findings"] });
  expect(scene.references.get(item.id)).toEqual({
    kind: "artifact",
    objective: "objective",
    execution: "execution",
    artifact: "artifact",
  });
  const nodes = reconcileNodes([], scene.items);
  nodes[0]!.position = { x: 800, y: 500 };
  expect(
    reconcileNodes(nodes, environmentResults(snapshot, [], projection).items)[0]!.position,
  ).toEqual({ x: 800, y: 500 });
  expect(environmentResults(snapshot, [], null).items).toHaveLength(0);
  expect(environmentResults({ ...snapshot, elements: [] }, [], projection).items).toHaveLength(0);
});
test("persisted results are not duplicated and transient results are bounded", () => {
  const reference = {
    kind: "artifact" as const,
    objective: "objective",
    execution: "execution",
    artifact: "artifact",
  };
  expect(
    environmentResults(
      { ...snapshot, elements: [...snapshot.elements, { id: "saved", area: null, reference }] },
      [],
      projection,
    ).items,
  ).toHaveLength(0);
  const many = structuredClone(projection);
  many.executions[0]!.artifacts = Array.from({ length: 30 }, (_, i) => ({
    ...projection.executions[0]!.artifacts[0]!,
    id: `result-${i}`,
  }));
  expect(environmentResults(snapshot, [], many).items).toHaveLength(12);
});

test("historical execution ownership keeps child research from crowding out root results", () => {
  const state = structuredClone(projection);
  const execution = state.executions[0]!;
  const root = execution.spec.nodes[0]!;
  execution.spec.nodes = [root, { ...root, node: "child", parent: root.node }];
  const artifact = execution.artifacts[0]!;
  execution.artifacts = [
    ...Array.from({ length: 20 }, (_, i) => ({
      ...artifact,
      node: "child",
      id: `child-${i}`,
      title: "Final recommendation",
    })),
    { ...artifact, id: "root-final", title: "Research notes" },
  ];
  state.work.plan = {
    author: "user",
    revision: "99",
    basis_revision: "4",
    draft: {
      id: "new-plan",
      nodes: [{ id: "child", objective: "New unrelated root", dependencies: [], outputs: [] }],
    },
  };
  const scene = environmentResults(snapshot, [], state);
  expect(scene.items.map((item) => item.id)).toEqual(["result:execution:root-final"]);
  expect(scene.remaining).toEqual({ objective: "objective", execution: "execution", count: 20 });
  const pinned = {
    kind: "artifact" as const,
    objective: "objective",
    execution: "execution",
    artifact: "child-0",
  };
  const saved = { id: "manual", title: "Manual child", kind: "Result", detail: "", status: "" };
  const withPinned = environmentResults(
    {
      ...snapshot,
      elements: [...snapshot.elements, { id: "manual", area: null, reference: pinned }],
    },
    [saved],
    state,
  );
  expect(withPinned.items[0]).toBe(saved);
  expect(withPinned.remaining?.count).toBe(19);
  expect(withPinned.references.get("manual")).toEqual(pinned);
});
test("newest root outputs win the cap while root overflow and unknown ownership remain counted", () => {
  const state = structuredClone(projection);
  const execution = state.executions[0]!;
  const artifact = execution.artifacts[0]!;
  execution.artifacts = [
    ...Array.from({ length: 15 }, (_, i) => ({ ...artifact, id: `root-${i}` })),
    { ...artifact, id: "unknown", node: "missing" },
  ];
  const scene = environmentResults(snapshot, [], state);
  expect(scene.items.map((item) => item.id)).toEqual(
    Array.from({ length: 12 }, (_, i) => `result:execution:root-${i + 3}`),
  );
  expect(scene.remaining?.count).toBe(4);
  const full = Array.from({ length: 500 }, (_, i) => ({
    id: `saved-${i}`,
    title: "Saved",
    kind: "Result",
    detail: "",
    status: "",
  }));
  const capacity = environmentResults(snapshot, full, state);
  expect(capacity.items).toEqual(full);
  expect(capacity.remaining?.count).toBe(16);
});

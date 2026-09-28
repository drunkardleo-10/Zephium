import { expect, test } from "vitest";
import type {
  WorkArtifactV1,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { environmentStages } from "../lib/project-environment-board";
import { environmentRequests } from "../lib/project-environment-thread";
import { projection, snapshot } from "./environment-fixtures";

type Data = WorkArtifactV1["data"];
const object = (id: string, data: Data, extra: Partial<WorkArtifactV1> = {}): WorkArtifactV1 => ({
  ...projection.executions[0]!.artifacts[0]!,
  id,
  title: "",
  data,
  ...extra,
});
const stay: Data = {
  kind: "picks",
  facet: "stay",
  items: [
    { name: "Sunny flat near Caltrain", price: { display: "$3,100 / month" }, recommended: true },
    { name: "Loft in SoMa", price: { display: "$3,600 / month" } },
  ],
};

/** A lead run: a Stay part that found two homes, and the reply and plan it made. */
function leadRun(): {
  scene: WorkEnvironmentSnapshot;
  objectives: Map<string, WorkRuntimeProjection>;
} {
  const state = structuredClone(projection);
  const run: WorkExecutionFact = state.executions[0]!;
  run.user_artifacts = [];
  run.parts = [
    {
      id: "stay",
      title: "Stay",
      helper: "browser",
      service: { host: "www.airbnb.com" },
      goal: "",
      state: "done",
      summary: "2 homes",
    },
    { id: "entry", title: "Entry", helper: "research", goal: "", state: "done" },
  ];
  run.artifacts = [
    object("reply", {
      kind: "reply",
      headline: "Your YC trip",
      text: "Fly LOT direct, stay in SoMa.",
      figures: [{ label: "Total", value: "$5,480" }],
      points: [],
    }),
    object("homes", stay, { part: "stay", title: "Homes" }),
    object(
      "plan",
      {
        kind: "plan",
        steps: [{ title: "Fly WAW → SFO", kind: "travel", when: "Jan 5" }],
        checkable: false,
      },
      { title: "Your trip" },
    ),
  ];
  const elements = ["reply", "homes", "plan"].map((id) => ({
    id: `${id}-card`,
    area: null,
    reference: {
      kind: "artifact" as const,
      objective: "objective",
      execution: "execution",
      artifact: id,
    },
  }));
  return {
    scene: { ...snapshot, elements: [snapshot.elements[0]!, ...elements] },
    objectives: new Map([["objective", state]]),
  };
}

test("a lead run's parts come from its facts, and what a part found ends its row", () => {
  const { scene, objectives } = leadRun();
  const [stage] = environmentStages(scene, objectives);
  expect(stage!.parts.map((part) => [part.title, part.helper, part.host])).toEqual([
    ["Stay", "browser", "airbnb.com"],
    ["Entry", "research", undefined],
  ]);
  const row = stage!.lane.rects[stage!.parts[0]!.id]!;
  const homes = stage!.lane.rects["homes-card"]!;
  expect(homes.y).toBe(row.y);
  expect(homes.x).toBeGreaterThan(row.x + row.width);
  // The reply heads the result; the plan stands under it.
  expect(stage!.reply?.view).toMatchObject({ kind: "reply", headline: "Your YC trip" });
  const head = stage!.lane.rects[stage!.column.head!]!;
  const plan = stage!.lane.rects["plan-card"]!;
  expect(plan.x).toBe(head.x);
  expect(plan.y).toBeGreaterThan(head.y + head.height);
  expect(head.x).toBeGreaterThan(homes.x + homes.width);
});

test("a revision shows where its first version stands, updated, and its request points at it", () => {
  const { scene, objectives } = leadRun();
  const state = objectives.get("objective")!;
  const first = state.executions[0]!;
  const followUp: WorkExecutionFact = {
    ...structuredClone(first),
    id: "01M3F2WPQC9VV4X4JFGSDZS7P8",
    spec: { ...first.spec, request: "Cheaper homes" },
    parts: [],
    artifacts: [
      object(
        "cheaper",
        { ...stay, items: [{ name: "Room in the Mission", price: { display: "$1,900 / month" } }] },
        { revises: "homes", execution: "01M3F2WPQC9VV4X4JFGSDZS7P8" },
      ),
    ],
  };
  state.executions = [first, followUp];
  scene.elements = [
    ...scene.elements,
    {
      id: "cheaper-card",
      area: null,
      reference: {
        kind: "artifact",
        objective: "objective",
        execution: followUp.id,
        artifact: "cheaper",
      },
    },
  ];
  const stages = environmentStages(scene, objectives);
  expect(stages).toHaveLength(2);
  const homes = stages[0]!.objects.find((entry) => entry.id === "homes-card")!;
  expect(homes.view).toMatchObject({ kind: "picks", items: [{ name: "Room in the Mission" }] });
  expect(homes.view.updated).toMatch(/^Updated · /u);
  expect(stages[1]!.objects.some((entry) => entry.id === "cheaper-card")).toBe(false);
  expect(stages[1]!.revised).toEqual(["homes-card"]);
  const links = environmentRequests(stages).links;
  expect(links.find((link) => link.id === `revise:${stages[1]!.card}:homes-card`)).toMatchObject({
    source: stages[1]!.card,
    target: "homes-card",
  });
});

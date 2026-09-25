import { expect, test } from "vitest";
import type { WorkExecutionFact, WorkEnvironmentSnapshot } from "$shared/ipc/bindings";
import { projection, snapshot } from "./environment-fixtures";
import { environmentAgents } from "../lib/project-environment";
import { environmentStages } from "../lib/project-environment-thread";

type Step = NonNullable<WorkExecutionFact["steps"]>[number];
const step = (id: string, kind: Step["kind"], status: Step["status"], artifacts?: string[]) =>
  ({ id, turn: 1, kind, status, ...(artifacts ? { artifacts } : {}) }) as Step;

function run(steps: Step[]) {
  const state = structuredClone(projection);
  const execution = state.executions[0]!;
  execution.status = "running";
  execution.authorization = "user_directed_agent";
  execution.spec.nodes[0]!.capability = {
    kind: "agent",
    grant: {
      provider: "open_ai",
      model: "gpt-5.6-luna",
      max_turns: 10,
      max_steps: 32,
      browse_hops: 4,
    },
  };
  execution.provider_evidence = [
    {
      id: "record",
      node: "node",
      attempt: "attempt",
      evidence: {
        version: 1,
        provider: "open_ai",
        model: "gpt-5.6-luna",
        response_model: "gpt-5.6-luna",
        response_id: "resp_1",
        search_call_id: "ws_1",
        answer: "One review",
        citations: [
          { url: "https://a.example/review", title: "Review A", start_index: 0, end_index: 3 },
        ],
        actual_input_tokens: 10,
        actual_output_tokens: 5,
      },
    },
  ];
  execution.artifacts = [
    ...execution.artifacts,
    {
      ...execution.artifacts[0]!,
      id: "sources",
      data: {
        kind: "evidence_collection",
        summary: "One review",
        subjects: [],
        entries: [{ evidence: 0, title: "Review A", role: "source", subject: null }],
      },
      evidence: [{ extraction_id: "record", source_id: 1 }],
    },
  ];
  execution.steps = steps;
  return new Map([["objective", state]]);
}

// Saved before lanes: the request takes its lane place instead.
const request = { x: 40, y: 60, width: 300, height: 110 };
function scene(published: boolean): WorkEnvironmentSnapshot {
  return {
    ...snapshot,
    elements: published ? snapshot.elements : snapshot.elements.slice(0, 1),
    view: {
      ...snapshot.view,
      placements: [{ element: "objective-card", ...request }],
    },
  };
}

test("the mark walks the lane: request, Worked with, the page it reads, Made, and the result", () => {
  const search = step("search", { kind: "search", query: "keyboards" }, "running", ["sources"]);
  const searched = { ...search, status: "succeeded" } as Step;
  const read = step("read", { kind: "read", url: "https://www.a.example/review" }, "running");
  const readDone = { ...read, status: "succeeded" } as Step;
  const publish = step("publish", { kind: "publish" }, "running", ["artifact"]);
  // Worked with holds Sources and one page: 24 + 300 + 16 + 248 + 24 wide, so Made starts at 1008.
  const made = 348 + 612 + 48;
  const cases: [string, Step[], boolean, { x: number; y: number }, string | undefined][] = [
    ["thinking", [step("turn", { kind: "turn" }, "running")], false, { x: 316, y: 0 }, "Thinking"],
    ["searching", [search], false, { x: 336, y: -12 }, "Searching"],
    [
      "reading",
      [searched, read],
      false,
      { x: 348 + 24 + 300 + 16 + 248 + 8, y: 44 - 8 - 24 },
      "Reading a.example",
    ],
    ["writing", [searched, readDone, publish], true, { x: made - 12, y: -12 }, "Writing"],
    [
      "done",
      [
        searched,
        readDone,
        { ...publish, status: "succeeded" } as Step,
        step("finish", { kind: "finish" }, "succeeded"),
      ],
      true,
      { x: made + 24 + 420 + 8, y: 44 - 8 - 24 },
      undefined,
    ],
  ];
  for (const [doing, steps, published, stand, caption] of cases) {
    const objectives = run(steps);
    const canvas = scene(published);
    const stages = environmentStages(canvas, objectives);
    const agents = environmentAgents(canvas, objectives, () => undefined, stages);
    const agent = agents.items[0]!;
    expect([agent.agent?.doing, agents.positions[agent.id], agent.agent?.caption]).toEqual([
      doing,
      stand,
      caption,
    ]);
    expect(agent.agent?.stand).toEqual(stand);
    // The mark is drawn from state; it ties itself to nothing.
    expect(agents.links).toEqual([]);
  }
});

import { expect, test } from "vitest";
import type { WorkExecutionFact, WorkRuntimeProjection } from "$shared/ipc/bindings";
import { projection, snapshot } from "./environment-fixtures";
import { environmentPages } from "../lib/project-environment";
import { environmentStages } from "../lib/project-environment-board";
import { runTrail } from "../lib/board/trail";
import { accountRefusal, accountRefusalSentence } from "../lib/work-human";

function agentRun(): { state: WorkRuntimeProjection; run: WorkExecutionFact } {
  const state = structuredClone(projection);
  const run = state.executions[0]!;
  run.spec.request = "What am I researching?";
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
  run.artifacts = [];
  run.user_artifacts = [];
  run.steps = [];
  // Pages stand in the process column while their run goes on.
  run.status = "running";
  return { state, run };
}
const scene = { ...snapshot, elements: snapshot.elements.slice(0, 1) };
const pagesOf = (state: WorkRuntimeProjection) => {
  const objectives = new Map([["objective", state]]);
  return environmentPages(objectives, environmentStages(scene, objectives), () => []);
};

test("consented open tabs stand as page cards without a read until one is read", () => {
  const { state, run } = agentRun();
  run.spec.context = {
    version: 1,
    environment: "work",
    environment_revision: "1",
    purpose: "agent",
    items: [],
    total_bytes: 0,
    tabs: [
      { title: "Sprint 42", host: "app.notion.com", path: "/p/Sprint-42" },
      { title: "Quiet keyboards", host: "shop.example", path: "/keyboards" },
    ],
  };
  const shown = pagesOf(state);
  expect(
    shown.map((item) => [
      item.title,
      item.status,
      item.page?.tab,
      item.page?.frame,
      item.unavailable,
    ]),
  ).toEqual([
    ["Sprint 42", "Tab", true, null, undefined],
    ["Quiet keyboards", "Tab", true, null, undefined],
  ]);
  // Shown is not read: the trail counts no pages.
  expect(runTrail([run], true).some((line) => line.icon === "page")).toBe(false);
  // A read of one takes its card over; the other stays a tab.
  run.steps = [
    {
      id: "read",
      turn: 1,
      kind: { kind: "read", url: "https://shop.example/keyboards" },
      status: "succeeded",
    },
  ];
  const read = pagesOf(state);
  expect(read.map((item) => [item.id, item.title, item.status, !!item.page?.tab])).toEqual([
    ["page:execution:read", "Quiet keyboards", "Read", false],
    ["page:execution:tab:0", "Sprint 42", "Tab", true],
  ]);
  expect(runTrail([run], true)[0]).toMatchObject({ icon: "page", text: "Read 1 page" });
});

test("a signed-in read carries its host to the card, and the trail its session use", () => {
  const { state, run } = agentRun();
  run.steps = [
    {
      id: "read",
      turn: 1,
      kind: { kind: "read", url: "https://app.notion.com/p/Sprint-42" },
      status: "succeeded",
      account: { host: "app.notion.com", badge: true },
    },
    {
      id: "public",
      turn: 1,
      kind: { kind: "read", url: "https://shop.example/keyboards" },
      status: "succeeded",
    },
  ];
  run.accounts = [{ host: "app.notion.com", pages_used: 1, pages: 12 }];
  expect(pagesOf(state).map((item) => item.page?.account)).toEqual(["app.notion.com", undefined]);
  expect(runTrail([run], true)).toContainEqual(
    expect.objectContaining({ text: "As you on app.notion.com", detail: "1 of 12 pages" }),
  );
});

test("signed-in refusals reach the line as plain sentences", () => {
  const { run } = agentRun();
  expect(accountRefusal(run)).toBeNull();
  run.accounts = [{ host: "app.notion.com", pages_used: 12, pages: 12 }];
  const spent = accountRefusal(run);
  expect(spent && accountRefusalSentence(spent)).toBe(
    "The agent used all 12 pages it was allowed on app.notion.com",
  );
  expect(accountRefusalSentence({ kind: "AccountWrite", host: "app.notion.com" })).toBe(
    "The agent stopped before changing something on app.notion.com",
  );
});

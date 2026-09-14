import fixture from "./approved.json";
import { expect, test } from "vitest";
import type { WorkRuntimeProjection, WorkSignalV1 } from "$shared/ipc/bindings";
import { currentActivity } from "../work-activity";

function running(): [WorkRuntimeProjection, WorkSignalV1] {
  const state = structuredClone(fixture) as WorkRuntimeProjection;
  const execution = state.executions[0]!;
  execution.status = "running";
  const node = execution.spec.nodes[0]!.node;
  const owner = "00000000000000000000000009";
  const attempt = "00000000000000000000000008";
  execution.attempts = [{ id: attempt, node, status: "running", usage: null }];
  state.owners = [{ execution: execution.id, owner }];
  return [
    state,
    {
      version: 1,
      owner,
      profile: state.work.profile,
      work: state.work.id,
      basis_revision: state.work.revision,
      execution: execution.id,
      attempt,
      node,
      activity: "reading",
    },
  ];
}

test("admits one activity per current owner/Work/execution/attempt/revision and rejects stale joins", () => {
  const [state, signal] = running();
  expect(currentActivity(state, [signal, signal])).toEqual([signal]);
  for (const key of [
    "owner",
    "profile",
    "work",
    "execution",
    "attempt",
    "node",
    "basis_revision",
  ] as const) {
    expect(currentActivity(state, [{ ...signal, [key]: "1" }])).toEqual([]);
  }
  state.work.revision = "9007199254740993";
  expect(currentActivity(state, [signal])).toEqual([]);
});

test("restart, terminal attempts and legacy projections never restore transient activity", () => {
  const [state, signal] = running();
  state.interrupted = [signal.execution];
  expect(currentActivity(state, [signal])).toEqual([]);
  state.interrupted = [];
  state.executions[0]!.attempts[0]!.status = "succeeded";
  expect(currentActivity(state, [signal])).toEqual([]);
  state.executions[0]!.attempts[0]!.status = "running";
  delete state.owners;
  expect(currentActivity(state, [signal])).toEqual([]);
});

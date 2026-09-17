import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { WorkSession } from "$domain/work";
import WorkFileInspector from "../components/WorkFileInspector.svelte";
import { projection } from "./environment-fixtures";
const native = vi.hoisted(() => ({ operation: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workOperation: native.operation });
});

test("a proposed edit shows both passages and the decision reaches the run", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = structuredClone(projection.executions[0]!);
  execution.status = "running";
  execution.authorization = "user_directed_agent";
  execution.steps = [
    {
      id: "edit-1",
      turn: 1,
      kind: {
        kind: "edit_file",
        path: "/Users/reader/notes/plan.md",
        old: "Monday: rest",
        new: "Monday: run",
      },
      status: "running",
    },
  ];
  session.projection = { ...structuredClone(projection), executions: [execution] };
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  const ondecided = vi.fn();
  const screen = await render(WorkFileInspector, {
    proposal: { session, step: "edit-1" },
    ondecided,
  });
  await expect.element(screen.getByText("~/notes/plan.md", { exact: true })).toBeVisible();
  await expect.element(screen.getByText("Monday: rest", { exact: true })).toBeVisible();
  await expect.element(screen.getByText("Monday: run", { exact: true })).toBeVisible();
  await screen.getByRole("button", { name: "Approve", exact: true }).click();
  expect(execute).toHaveBeenCalledExactlyOnceWith({
    kind: "approve_step",
    execution: "execution",
    step: "edit-1",
    approve: true,
  });
  await expect.poll(() => ondecided.mock.calls.length).toBe(1);
  await screen.unmount();
  session.dispose();
});

test("a settled change shows what was applied, and a declined one says so", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = structuredClone(projection.executions[0]!);
  execution.file_evidence = [
    {
      id: "record",
      node: "node",
      attempt: "attempt",
      file: {
        path: "/Users/reader/notes/plan.md",
        name: "plan.md",
        kind: "written",
        bytes: 12,
        digest: "abc",
        text: "- Monday: rest\n+ Monday: run",
        truncated: false,
      },
    },
  ];
  execution.steps = [
    {
      id: "edit-1",
      turn: 1,
      kind: {
        kind: "edit_file",
        path: "/Users/reader/notes/plan.md",
        old: "Monday: rest",
        new: "Monday: run",
        decision: true,
      },
      status: "succeeded",
      evidence: "record",
    },
    {
      id: "edit-2",
      turn: 1,
      kind: {
        kind: "write_file",
        path: "/Users/reader/notes/next.md",
        content: "Tuesday",
        decision: false,
      },
      status: "failed",
      note: "Declined by the person",
    },
  ];
  session.projection = { ...structuredClone(projection), executions: [execution] };
  const applied = await render(WorkFileInspector, { proposal: { session, step: "edit-1" } });
  await expect
    .element(applied.getByText("- Monday: rest\n+ Monday: run", { exact: true }))
    .toBeVisible();
  await expect
    .element(applied.getByRole("button", { name: "Approve", exact: true }))
    .not.toBeInTheDocument();
  await applied.unmount();
  const refused = await render(WorkFileInspector, { proposal: { session, step: "edit-2" } });
  await expect.element(refused.getByText("Declined.", { exact: true })).toBeVisible();
  await refused.unmount();
  session.dispose();
});

import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { WorkSession } from "$domain/work";
import type { WorkExecutionFact } from "$shared/ipc/bindings";
import AgentLine from "../components/AgentLine.svelte";
import { projection } from "./environment-fixtures";
const native = vi.hoisted(() => ({ operation: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workOperation: native.operation });
});

function agentRun(status: WorkExecutionFact["status"]): WorkExecutionFact {
  const base = structuredClone(projection.executions[0]!);
  return {
    ...base,
    status,
    authorization: "user_directed_agent",
    attempts: [{ id: "attempt", node: "node", status: "running", usage: null }],
    spec: {
      ...base.spec,
      nodes: [
        {
          ...base.spec.nodes[0]!,
          capability: {
            kind: "agent",
            grant: {
              provider: "open_ai",
              model: "gpt-5.6-luna",
              max_turns: 10,
              max_steps: 32,
              browse_hops: 4,
            },
          },
        },
      ],
    },
    steps: [
      {
        id: "turn-1",
        turn: 1,
        kind: { kind: "turn" },
        status: "succeeded",
        usage: { model_tokens: 10, cost_micro_usd: 1, operations: 1, accounting: "exact" },
        note: "Looking for quiet keyboards.",
      },
      {
        id: "search-1",
        turn: 1,
        kind: { kind: "search", query: "quiet mechanical keyboards 2026" },
        status: "succeeded",
        usage: { model_tokens: 10, cost_micro_usd: 1, operations: 1, accounting: "exact" },
        note: "Found 6 sources",
      },
    ],
  };
}

test("a running agent says only what it is doing and answers its question in place", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = agentRun("running");
  execution.steps = [
    ...execution.steps!,
    {
      id: "read-1",
      turn: 2,
      kind: { kind: "read", url: "https://lego.com/sets" },
      status: "running",
    },
    {
      id: "ask-1",
      turn: 2,
      kind: { kind: "ask", prompt: "Which budget?", options: ["Under 150", "Under 300"] },
      status: "running",
    },
  ];
  session.projection = {
    ...structuredClone(projection),
    executions: [execution],
    owners: [{ execution: execution.id, owner: "owner" }],
  };
  session.activity = [
    {
      version: 1,
      owner: "owner",
      profile: "profile",
      work: "objective",
      basis_revision: session.projection.work.revision,
      execution: execution.id,
      attempt: "attempt",
      node: "node",
      activity: "reading",
    },
  ];
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  vi.spyOn(session.operations, "busy").mockReturnValue(true);
  const screen = await render(AgentLine, { session });
  await expect.element(screen.getByText("Reading lego.com", { exact: true })).toBeVisible();
  // The line is the state and nothing else: no step list, no counts, no objective.
  expect(screen.container.textContent).not.toContain("Found 6 sources");
  expect(screen.container.textContent).not.toContain("Looking for quiet keyboards.");
  expect(screen.container.textContent).not.toContain(projection.work.objective);
  await screen.getByRole("button", { name: "Answer", exact: true }).click();
  await screen.getByRole("button", { name: "Under 150", exact: true }).click();
  await screen.getByRole("button", { name: "Send answer", exact: true }).click();
  expect(execute).toHaveBeenCalledExactlyOnceWith({
    kind: "answer_step",
    execution: "execution",
    step: "ask-1",
    answer: "Under 150",
  });
  await screen.unmount();
  session.dispose();
});

test("a finished run closes on its own sentence and a follow-up continues the work", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const done = agentRun("needs_review");
  done.steps = [
    ...done.steps!,
    {
      id: "finish",
      turn: 2,
      kind: { kind: "finish", followups: ["Show me the quietest one"] },
      status: "succeeded",
      note: "Compared three keyboards on price and switch noise.",
    },
  ];
  session.projection = { ...structuredClone(projection), executions: [done] };
  const continueWith = vi.spyOn(session, "continueWith").mockResolvedValue(true);
  const screen = await render(AgentLine, { session });
  await expect
    .element(
      screen.getByText("Compared three keyboards on price and switch noise.", { exact: true }),
    )
    .toBeVisible();
  await expect
    .element(screen.getByRole("button", { name: "Stop", exact: true }))
    .not.toBeInTheDocument();
  await screen.getByRole("button", { name: "Show me the quietest one", exact: true }).click();
  expect(continueWith).toHaveBeenCalledExactlyOnceWith("Show me the quietest one");
  await screen.unmount();
  session.dispose();
});

test("a run stopped on its question keeps asking it, and the answer continues the work", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const stopped = agentRun("cancelled");
  stopped.steps = [
    ...stopped.steps!,
    {
      id: "ask-1",
      turn: 2,
      kind: {
        kind: "ask",
        prompt: "No monthly totals without dates. What next?",
        options: ["Use sample dates", "Skip totals"],
      },
      status: "cancelled",
      note: "Waiting for your answer",
    },
  ];
  session.projection = { ...structuredClone(projection), executions: [stopped] };
  const continueWith = vi.spyOn(session, "continueWith").mockResolvedValue(true);
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  const screen = await render(AgentLine, { session });
  await expect.element(screen.getByText("Waiting for you", { exact: true })).toBeVisible();
  expect(screen.container.textContent).not.toContain("Something went wrong");
  await screen.getByRole("button", { name: "Answer", exact: true }).click();
  await screen.getByRole("button", { name: "Use sample dates", exact: true }).click();
  await screen.getByRole("button", { name: "Send answer", exact: true }).click();
  expect(continueWith).toHaveBeenCalledExactlyOnceWith("Use sample dates");
  expect(execute).not.toHaveBeenCalled();
  await screen.unmount();
  session.dispose();
});

test("an accepted steer reaches the running execution without queueing", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = {
    ...structuredClone(projection),
    executions: [agentRun("running")],
  };
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  const screen = await render(AgentLine, { session, draft: "Prefer linear switches" });
  await screen.getByRole("button", { name: "Steer", exact: true }).click();
  expect(execute).toHaveBeenCalledExactlyOnceWith({
    kind: "steer",
    execution: "execution",
    text: "Prefer linear switches",
  });
  expect(session.queue).toEqual([]);
  await screen.unmount();
  session.dispose();
});

test("a refused steer queues the draft and the queue rides on when the run closes", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = agentRun("running");
  session.projection = { ...structuredClone(projection), executions: [execution] };
  const continueWith = vi.spyOn(session, "continueWith").mockResolvedValue(true);
  // A refused steer must not drop the message: it waits for the run instead.
  const steer = vi.spyOn(session, "steer").mockResolvedValue(false);
  const onsteered = vi.fn();
  const screen = await render(AgentLine, {
    session,
    draft: "Also compare the wireless ones",
    onsteered,
  });
  await expect
    .element(screen.getByText("Also compare the wireless ones", { exact: true }))
    .toBeVisible();
  await screen.getByRole("button", { name: "Steer", exact: true }).click();
  expect(steer).toHaveBeenCalledExactlyOnceWith("Also compare the wireless ones");
  await expect.poll(() => session.queue).toEqual(["Also compare the wireless ones"]);
  expect(onsteered).toHaveBeenCalledOnce();
  expect(continueWith).not.toHaveBeenCalled();
  session.projection = {
    ...session.projection,
    executions: [{ ...execution, status: "completed" }],
  };
  await expect.poll(() => continueWith.mock.calls).toEqual([["Also compare the wireless ones"]]);
  expect(session.queue).toEqual([]);
  await screen.unmount();
  session.dispose();
});

test("a proposed file change waits on the person and hands the review to the canvas", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = agentRun("running");
  execution.steps = [
    ...execution.steps!,
    {
      id: "edit-1",
      turn: 2,
      kind: { kind: "edit_file", path: "/Users/reader/notes/plan.md", old: "a", new: "b" },
      status: "running",
    },
  ];
  session.projection = { ...structuredClone(projection), executions: [execution] };
  const onreview = vi.fn();
  const screen = await render(AgentLine, { session, onreview });
  await expect.element(screen.getByText("Wants to change plan.md", { exact: true })).toBeVisible();
  // The line never holds the change itself: it opens where the person can read it.
  await screen.getByRole("button", { name: "Review", exact: true }).click();
  expect(onreview).toHaveBeenCalledExactlyOnceWith("edit-1");
  expect(screen.container.textContent).not.toContain("/Users/reader/notes/plan.md");
  await screen.unmount();
  session.dispose();
});

test("a failed run says why it gave up, and a stopped one just stops", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const failed = agentRun("failed");
  failed.steps = [
    ...failed.steps!,
    {
      id: "read-1",
      turn: 2,
      kind: { kind: "read", url: "https://lego.com/sets" },
      status: "failed",
      note: "The page asked for a human check",
    },
  ];
  session.projection = { ...structuredClone(projection), executions: [failed] };
  const screen = await render(AgentLine, { session });
  await expect
    .element(screen.getByText("The page asked for a human check", { exact: true }))
    .toBeVisible();
  expect(screen.container.textContent).not.toContain("Something went wrong");
  await screen.unmount();
  const stopped = new WorkSession("profile");
  stopped.selected = "objective";
  stopped.projection = { ...structuredClone(projection), executions: [agentRun("cancelled")] };
  const cancelled = await render(AgentLine, { session: stopped });
  await expect.element(cancelled.getByText("Stopped.", { exact: true })).toBeVisible();
  await cancelled.unmount();
  session.dispose();
  stopped.dispose();
});

test("the line says which page is waiting for you and links to its card", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = agentRun("running");
  execution.steps = [
    ...execution.steps!,
    {
      id: "read-1",
      turn: 2,
      kind: { kind: "read", url: "https://ferry.example/book" },
      status: "running",
    },
  ];
  session.projection = { ...structuredClone(projection), executions: [execution] };
  const onwaitingpage = vi.fn();
  const screen = await render(AgentLine, {
    session,
    waiting: { card: "page:execution:read-1", host: "ferry.example", remaining: 150_000 },
    onwaitingpage,
  });
  const link = screen.getByRole("button", { name: /Waiting for you on ferry.example/ });
  await expect.element(link).toBeVisible();
  // A long wait stays quiet; the countdown belongs to the last minute.
  expect(screen.container.textContent).not.toContain("s left");
  await screen.rerender({
    session,
    waiting: { card: "page:execution:read-1", host: "ferry.example", remaining: 18_000 },
    onwaitingpage,
  });
  await expect.element(screen.getByText("18s left")).toBeVisible();
  await link.click();
  expect(onwaitingpage).toHaveBeenCalledExactlyOnceWith("page:execution:read-1");
  await screen.unmount();
  session.dispose();
});

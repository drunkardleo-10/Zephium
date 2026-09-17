import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { WorkSession } from "$domain/work";
import WorkInteraction from "../components/WorkInteraction.svelte";
import type { WorkExecutionFact } from "$shared/ipc/bindings";
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
      {
        id: "ask-1",
        turn: 2,
        kind: { kind: "ask", prompt: "Which budget?", options: ["Under 150", "Under 300"] },
        status: "running",
      },
    ],
  };
}

test("a running agent shows its line, recent steps, and answers its question in place", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = { ...structuredClone(projection), executions: [agentRun("running")] };
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  // The run itself is one busy operation; its questions stay answerable.
  vi.spyOn(session.operations, "busy").mockReturnValue(true);
  const screen = await render(WorkInteraction, { session, ondetails: vi.fn() });
  await expect
    .element(screen.getByText("Looking for quiet keyboards.", { exact: true }))
    .toBeVisible();
  await expect.element(screen.getByText("Found 6 sources", { exact: true })).toBeVisible();
  await expect
    .element(screen.getByRole("button", { name: "Plan details", exact: true }))
    .not.toBeInTheDocument();
  await expect
    .element(screen.getByRole("button", { name: "Prepare execution", exact: true }))
    .not.toBeInTheDocument();
  await screen.getByRole("button", { name: "Under 150", exact: true }).click();
  await screen.getByRole("button", { name: "Continue", exact: true }).click();
  expect(execute).toHaveBeenCalledExactlyOnceWith({
    kind: "answer_step",
    execution: "execution",
    step: "ask-1",
    answer: "Under 150",
  });
  expect(screen.container.querySelector(".settled")).toBeNull();
  await screen.unmount();
  session.dispose();
});

test("a finished agent run settles to its closing line and unknown delivery offers a refresh", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const done = agentRun("needs_review");
  done.attempts = [
    {
      id: "attempt",
      node: "node",
      status: "succeeded",
      usage: { model_tokens: 20, cost_micro_usd: 2, operations: 2, accounting: "exact" },
    },
  ];
  // The finishing turn says nothing on its turn step; the finish step carries
  // the closing sentence, and that is the line a settled run ends on.
  done.steps = [
    ...done.steps!.slice(0, 2),
    {
      id: "turn-2",
      turn: 2,
      kind: { kind: "turn" },
      status: "succeeded",
      usage: { model_tokens: 10, cost_micro_usd: 1, operations: 1, accounting: "exact" },
    },
    {
      id: "finish",
      turn: 2,
      kind: { kind: "finish" },
      status: "succeeded",
      note: "Compared three keyboards on price and switch noise.",
    },
  ];
  session.projection = { ...structuredClone(projection), executions: [done] };
  const screen = await render(WorkInteraction, { session, ondetails: vi.fn() });
  await expect.poll(() => screen.container.querySelector(".settled")).not.toBeNull();
  expect(screen.container.textContent).toContain(
    "Compared three keyboards on price and switch noise.",
  );
  expect(screen.container.textContent).not.toContain("Looking for quiet keyboards.");
  expect(screen.container.textContent).not.toContain(projection.work.objective);
  session.delivery = "unknown";
  await expect
    .element(screen.getByRole("button", { name: "Refresh current state", exact: true }))
    .toBeVisible();
  expect(screen.container.querySelector(".settled")).toBeNull();
  await screen.unmount();
  session.dispose();
});

test("stale-owner running facts require interruption review", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = agentRun("running");
  session.projection = {
    ...structuredClone(projection),
    executions: [execution],
    interrupted: [execution.id],
  };
  const execute = vi.spyOn(session, "execute").mockResolvedValue(false);
  const ondetails = vi.fn();
  const screen = await render(WorkInteraction, { session, ondetails });
  await expect
    .element(screen.getByRole("button", { name: "Continue", exact: true }))
    .not.toBeInTheDocument();
  await screen.getByRole("button", { name: "Review interruption", exact: true }).click();
  expect(ondetails).toHaveBeenCalledExactlyOnceWith(execution.id);
  expect(execute).not.toHaveBeenCalled();
  await screen.unmount();
  session.dispose();
});

test.each(["unknown", "refused"] as const)(
  "a %s run admission stays visible without a new admission",
  async (outcome) => {
    const session = new WorkSession("profile");
    session.selected = "objective";
    session.projection = {
      ...structuredClone(projection),
      executions: [],
      work: { ...projection.work, status: "draft", plan: null },
    };
    native.operation.mockImplementation(async (profile: string, operation: string) => ({
      version: 1,
      profile,
      operation,
      state: outcome === "unknown" ? { kind: "unknown" } : { kind: "refused", error: "invalid" },
    }));
    await session.run();
    const screen = await render(WorkInteraction, { session, ondetails: vi.fn() });
    if (outcome === "unknown")
      await expect
        .element(screen.getByRole("button", { name: "Refresh current state", exact: true }))
        .toBeVisible();
    else
      await expect
        .element(screen.getByText("Work could not confirm this request: invalid", { exact: true }))
        .toBeVisible();
    await expect
      .element(screen.getByRole("button", { name: "Approve this plan and scope", exact: true }))
      .not.toBeInTheDocument();
    await screen.unmount();
    session.dispose();
  },
);

test("live browser phases render from the current attempt and disappear on completion", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = agentRun("running");
  execution.steps = execution.steps?.slice(0, 2);
  session.projection = {
    ...structuredClone(projection),
    executions: [execution],
    owners: [{ execution: execution.id, owner: "owner" }],
  };
  const screen = await render(WorkInteraction, { session, ondetails: vi.fn() });
  for (const [activity, label] of [
    ["interacting", "Interacting with the page"],
    ["verifying", "Checking the result"],
    ["recovering", "Recovering the browser"],
  ] as const) {
    session.activity = [
      {
        version: 1,
        owner: "owner",
        profile: session.projection.work.profile,
        work: session.projection.work.id,
        basis_revision: session.projection.work.revision,
        execution: execution.id,
        attempt: "attempt",
        node: "node",
        activity,
      },
    ];
    await expect.element(screen.getByText(label, { exact: true })).toBeVisible();
  }
  session.projection = {
    ...session.projection,
    executions: [{ ...execution, status: "needs_review" }],
  };
  await expect
    .element(screen.getByText("Recovering the browser", { exact: true }))
    .not.toBeInTheDocument();
  await screen.unmount();
  session.dispose();
});

import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { WorkSession } from "$domain/work";
import WorkInteraction from "../components/WorkInteraction.svelte";
import type { WorkOperationStateV1 } from "$shared/ipc/bindings";
import { projection } from "./environment-fixtures";
const native = vi.hoisted(() => ({ operation: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workOperation: native.operation });
});
test("settled work stays compact while unknown and running work retain controls", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = structuredClone(projection);
  const ondetails = vi.fn();
  const screen = await render(WorkInteraction, { session, ondetails });
  expect(screen.container.textContent).not.toContain(projection.work.objective);
  await expect
    .element(screen.getByRole("button", { name: "Prepare execution", exact: true }))
    .not.toBeInTheDocument();
  await screen.getByRole("button", { name: "Plan details", exact: true }).click();
  expect(ondetails).toHaveBeenCalledOnce();
  session.delivery = "unknown";
  await expect
    .element(screen.getByRole("button", { name: "Refresh current state", exact: true }))
    .toBeVisible();
  expect(screen.container.querySelector(".settled")).toBeNull();
  session.delivery = "ready";
  session.projection = {
    ...session.projection!,
    executions: [{ ...session.projection!.executions[0]!, status: "running" }],
  };
  await expect
    .element(screen.getByRole("button", { name: "Cancel execution", exact: true }))
    .toBeVisible();
  expect(screen.container.querySelector(".settled")).toBeNull();
  await screen.unmount();
  session.dispose();
});

test.each(["failed", "interrupted"] as const)(
  "%s work can prepare again without approving or starting execution",
  async (status) => {
    const session = new WorkSession("profile");
    session.selected = "objective";
    session.projection = {
      ...structuredClone(projection),
      executions: [{ ...structuredClone(projection.executions[0]!), status }],
    };
    const prepare = vi.spyOn(session.operations, "begin").mockResolvedValue();
    const execute = vi.spyOn(session, "execute").mockResolvedValue(false);
    const screen = await render(WorkInteraction, { session, ondetails: vi.fn() });
    await screen.getByRole("button", { name: "Prepare execution", exact: true }).click();
    expect(prepare).toHaveBeenCalledExactlyOnceWith({
      kind: "prepare_plan",
      request: { version: 1, work: "objective", expected_revision: "4" },
    });
    expect(execute).not.toHaveBeenCalled();
    expect(screen.container.querySelector(".settled")).not.toBeNull();
    await screen.unmount();
    session.dispose();
  },
);

test.each([
  "refused",
  "planned-refusal",
  "planned-error",
  "settled-error",
  "unknown",
  "approval",
] as const)(
  "real preparation response %s remains inspectable after failed execution",
  async (outcome) => {
    const session = new WorkSession("profile");
    session.selected = "objective";
    const previous = structuredClone(projection.executions[0]!);
    session.projection = {
      ...structuredClone(projection),
      work: {
        ...projection.work,
        plan: {
          author: "user",
          revision: "2",
          basis_revision: "1",
          draft: { id: "plan", nodes: [] },
        },
      },
      executions: [{ ...previous, status: "failed" }],
    };
    let state: WorkOperationStateV1;
    if (outcome === "refused") state = { kind: "refused", error: "invalid" };
    else if (outcome === "unknown") state = { kind: "unknown" };
    else if (outcome === "settled-error")
      state = {
        kind: "settled",
        response: { version: 1, profile: "profile", reply: { kind: "error", error: "conflict" } },
      };
    else
      state = {
        kind: "planned",
        response: {
          version: 1,
          profile: "profile",
          work: "objective",
          basis_revision: "4",
          usage: null,
          outcome:
            outcome === "planned-refusal"
              ? { kind: "refused", reason: { kind: "provider_refused" } }
              : {
                  kind: "settled",
                  response: {
                    version: 1,
                    profile: "profile",
                    reply:
                      outcome === "approval"
                        ? {
                            kind: "approval_draft",
                            work: "objective",
                            expected_revision: "4",
                            spec: previous.spec,
                          }
                        : { kind: "error", error: "conflict" },
                  },
                },
        },
      };
    native.operation.mockImplementation(async (profile: string, operation: string) => ({
      version: 1,
      profile,
      operation,
      state,
    }));
    const execute = vi.spyOn(session, "execute").mockResolvedValue(false);
    const screen = await render(WorkInteraction, { session, ondetails: vi.fn() });
    await screen.getByRole("button", { name: "Prepare execution", exact: true }).click();
    await expect.poll(() => screen.container.querySelector(".settled")).toBeNull();
    if (outcome === "approval") {
      await screen.getByText("Review execution scope", { exact: true }).click();
      await expect
        .element(screen.getByRole("button", { name: "Approve this plan and scope", exact: true }))
        .toBeVisible();
    } else if (outcome === "unknown")
      await expect
        .element(
          screen.getByText(
            "The operation outcome is unknown. Refresh Work before deciding what to do next.",
            {
              exact: true,
            },
          ),
        )
        .toBeVisible();
    else await expect.element(screen.getByRole("alert")).toBeVisible();
    expect(execute).not.toHaveBeenCalled();
    await screen.unmount();
    session.dispose();
  },
);

test("stale-owner running facts require interruption review before preparation", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = { ...structuredClone(projection.executions[0]!), status: "running" as const };
  session.projection = {
    ...structuredClone(projection),
    executions: [execution],
    interrupted: [execution.id],
  };
  const prepare = vi.spyOn(session.operations, "begin").mockResolvedValue();
  const execute = vi.spyOn(session, "execute").mockResolvedValue(false);
  const ondetails = vi.fn();
  const screen = await render(WorkInteraction, { session, ondetails });
  await expect
    .element(screen.getByRole("button", { name: "Prepare execution", exact: true }))
    .not.toBeInTheDocument();
  await expect
    .element(screen.getByRole("button", { name: "Cancel execution", exact: true }))
    .not.toBeInTheDocument();
  await screen.getByRole("button", { name: "Review interruption", exact: true }).click();
  expect(ondetails).toHaveBeenCalledExactlyOnceWith(execution.id);
  expect(prepare).not.toHaveBeenCalled();
  expect(execute).not.toHaveBeenCalled();
  await screen.unmount();
  session.dispose();
});

test.each(["unknown", "refused"] as const)(
  "public research %s stays visible without a new admission",
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
    await session.readPublic();
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
      .element(screen.getByRole("button", { name: "Prepare execution", exact: true }))
      .not.toBeInTheDocument();
    await expect
      .element(screen.getByRole("button", { name: "Approve this plan and scope", exact: true }))
      .not.toBeInTheDocument();
    await screen.unmount();
    session.dispose();
  },
);

test("admitted public research never offers ordinary Start after a lost dispatch", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = {
    ...structuredClone(projection),
    executions: [
      {
        ...structuredClone(projection.executions[0]!),
        status: "approved",
        authorization: "user_directed_public_read",
      },
    ],
  };
  const screen = await render(WorkInteraction, { session, ondetails: vi.fn() });
  await expect
    .element(screen.getByRole("button", { name: "Start execution", exact: true }))
    .not.toBeInTheDocument();
  await expect
    .element(
      screen.getByText(
        "Public research has not started. Review or cancel this execution.",
        { exact: true },
      ),
    )
    .toBeVisible();
  await expect
    .element(screen.getByRole("button", { name: "Cancel execution", exact: true }))
    .toBeVisible();
  await screen.unmount();
  session.dispose();
});

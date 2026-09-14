import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { WorkSession } from "$domain/work";
import type { WorkExecutionFact } from "$shared/ipc/bindings";
import WorkInteraction from "../components/WorkInteraction.svelte";
import { projection } from "./environment-fixtures";
const native = vi.hoisted(() => ({ operation: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workOperation: native.operation });
});

const scope = {
  tab: "tab-1",
  url: "https://app.notion.com/p/Sprint-42",
  origin: "https://app.notion.com",
  account: "01JACCOUNT0000000000000000",
};
function accountExecution(status: WorkExecutionFact["status"]): WorkExecutionFact {
  const previous = structuredClone(projection.executions[0]!);
  return {
    ...previous,
    status,
    artifacts: [],
    user_artifacts: [],
    spec: {
      ...previous.spec,
      nodes: [{ ...previous.spec.nodes[0]!, capability: { kind: "account_read", scope } }],
    },
  };
}

test("a sign-in intervention offers the page and a fresh run under the same approval", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
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
    executions: [
      { ...accountExecution("failed"), intervention: { kind: "sign_in", origin: scope.origin } },
    ],
  };
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  const onopenpage = vi.fn();
  const screen = await render(WorkInteraction, { session, ondetails: vi.fn(), onopenpage });
  await expect
    .element(screen.getByText("Sign in is needed on https://app.notion.com.", { exact: true }))
    .toBeVisible();
  await screen.getByRole("button", { name: "Open page", exact: true }).click();
  expect(onopenpage).toHaveBeenCalledExactlyOnceWith("tab-1");
  await screen.getByRole("button", { name: "Run again", exact: true }).click();
  expect(execute).toHaveBeenCalledExactlyOnceWith({
    kind: "approve",
    spec: session.projection!.executions[0]!.spec,
  });
  await screen.unmount();
  session.dispose();
});

test("taking over a running signed-in page revokes automation with a persisted reason", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = {
    ...structuredClone(projection),
    executions: [accountExecution("running")],
  };
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  const onopenpage = vi.fn();
  const screen = await render(WorkInteraction, { session, ondetails: vi.fn(), onopenpage });
  await screen.getByRole("button", { name: "Take over", exact: true }).click();
  expect(execute).toHaveBeenCalledExactlyOnceWith({
    kind: "cancel",
    execution: "execution",
    intervention: { kind: "human_takeover", origin: "https://app.notion.com" },
  });
  await expect.poll(() => onopenpage.mock.calls).toEqual([["tab-1"]]);
  await expect
    .element(screen.getByRole("button", { name: "Run again", exact: true }))
    .not.toBeInTheDocument();
  await screen.unmount();
  session.dispose();
});

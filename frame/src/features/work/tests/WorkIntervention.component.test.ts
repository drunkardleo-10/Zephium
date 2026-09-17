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

test("a sign-in intervention hands the signed-in page over without revoking a settled run", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = {
    ...structuredClone(projection),
    executions: [
      { ...accountExecution("failed"), intervention: { kind: "sign_in", origin: scope.origin } },
    ],
  };
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  const onopenpage = vi.fn();
  const screen = await render(AgentLine, { session, onopenpage });
  await screen.getByRole("button", { name: "Open", exact: true }).click();
  await expect.poll(() => onopenpage.mock.calls).toEqual([["tab-1"]]);
  expect(execute).not.toHaveBeenCalled();
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
  const screen = await render(AgentLine, { session, onopenpage });
  await screen.getByRole("button", { name: "Open", exact: true }).click();
  expect(execute).toHaveBeenCalledExactlyOnceWith({
    kind: "cancel",
    execution: "execution",
    intervention: { kind: "human_takeover", origin: "https://app.notion.com" },
  });
  await expect.poll(() => onopenpage.mock.calls).toEqual([["tab-1"]]);
  await screen.unmount();
  session.dispose();
});

import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { WorkSession } from "$domain/work";
import type {
  WorkAccountGrantV1,
  WorkContextSelectionV1,
  WorkExecutionFact,
  WorkOperationV1,
} from "$shared/ipc/bindings";
import WorkExecutionReview from "../components/WorkExecutionReview.svelte";
import AgentLine from "../components/AgentLine.svelte";
import PageCard from "../components/cards/PageCard.svelte";
import ObjectiveCard from "../components/cards/ObjectiveCard.svelte";
import type { CanvasItem } from "../lib/canvas-model";
import { projection } from "./environment-fixtures";

const native = vi.hoisted(() => ({ call: vi.fn(), operation: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call, workOperation: native.operation });
});

const grant: WorkAccountGrantV1 = {
  origin: "https://app.notion.com",
  account: "01JACCOUNT0000000000000000",
  tab: "tab-1",
  pages: 12,
};

/** A work whose origin grant Rust drafted and the person has not answered yet. */
async function drafted(context: WorkContextSelectionV1 | null) {
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = { ...structuredClone(projection), executions: [] };
  const begin = vi
    .spyOn(session.operations, "begin")
    .mockImplementationOnce(async (input: WorkOperationV1) => {
      session.operations.jobs.set("grant", {
        id: "grant",
        input,
        state: {
          kind: "settled",
          response: {
            version: 1,
            profile: "profile",
            reply: { kind: "account_grant_draft", work: "objective", grant },
          },
        },
      });
    })
    .mockResolvedValue();
  await session.prepareGrant(
    {
      version: 1,
      work: "objective",
      expected_revision: "4",
      environment: "environment",
      element: "tab-element",
    },
    context,
  );
  return { session, begin };
}

test("an origin grant asks in plain words and rides with the run once allowed", async () => {
  const context = { environment: "environment", items: [], tabs: true };
  const { session, begin } = await drafted(context);
  expect(begin.mock.calls[0]![0]).toEqual({
    kind: "prepare_account",
    request: {
      version: 1,
      work: "objective",
      expected_revision: "4",
      environment: "environment",
      element: "tab-element",
      effect: { kind: "read" },
      mode: "origin",
    },
  });
  const screen = await render(WorkExecutionReview, { session });
  const review = screen.getByRole("region", {
    name: "Let the agent read app.notion.com as me for this request",
  });
  await expect
    .element(review.getByText("Read app.notion.com as you for this request?", { exact: true }))
    .toBeVisible();
  await expect
    .element(review.getByText("up to 12 pages, reading only", { exact: true }))
    .toBeVisible();
  // The attested account is never shown: not a name, not the minted id.
  expect(screen.container.textContent).not.toContain(grant.account);
  await review.getByRole("button", { name: "Allow for this request", exact: true }).click();
  await expect.poll(() => begin.mock.calls.length).toBe(2);
  const run = begin.mock.calls[1]![0];
  expect(run.kind).toBe("run");
  if (run.kind !== "run" || run.command.intent.kind !== "begin_agent") throw new Error("run");
  expect(run.context).toEqual(context);
  expect(run.command.intent.grant.accounts).toEqual([grant]);
  // Allowed once: the review is gone and the grant is not offered again.
  expect(session.grantDraft).toBeNull();
  await expect.element(review).not.toBeInTheDocument();
  await screen.unmount();
  session.dispose();
});

test("not now forgets the drafted grant and runs without the session", async () => {
  const { session, begin } = await drafted(null);
  const screen = await render(WorkExecutionReview, { session });
  await screen.getByRole("button", { name: "Not now", exact: true }).click();
  await expect
    .element(screen.getByRole("button", { name: "Allow for this request" }))
    .not.toBeInTheDocument();
  await expect.poll(() => begin.mock.calls.length).toBe(2);
  const run = begin.mock.calls[1]![0];
  if (run.kind !== "run" || run.command.intent.kind !== "begin_agent") throw new Error("run");
  expect(run.command.intent.grant.accounts).toBeUndefined();
  expect(session.grantDraft).toBeNull();
  await screen.unmount();
  session.dispose();
});

function pageItem(page: Partial<NonNullable<CanvasItem["page"]>>, status = "Read"): CanvasItem {
  return {
    id: "page",
    type: "page",
    kind: "Page",
    title: "Sprint 42",
    detail: "https://app.notion.com/p/Sprint-42",
    status,
    page: {
      url: "https://app.notion.com/p/Sprint-42",
      host: "app.notion.com",
      frame: null,
      live: false,
      ...page,
    },
  };
}

test("a page read under a grant carries the account badge beside its mark", async () => {
  await page.viewport(1200, 800);
  const screen = await render(PageCard, {
    item: pageItem({ account: "app.notion.com" }),
    selected: false,
  });
  const badge = screen.getByRole("img", { name: "Read as you on app.notion.com", exact: true });
  await expect.element(badge).toBeVisible();
  await expect.element(badge).toHaveAttribute("title", "Read as you on app.notion.com");
  expect(screen.container.querySelector(".about .marks .account-badge")).not.toBeNull();
  await screen.rerender({ item: pageItem({}), selected: false });
  expect(screen.container.querySelector(".account-badge")).toBeNull();
  await screen.unmount();
});

test("an open tab the request was shown stands as a page card without a frame", async () => {
  await page.viewport(1200, 800);
  const screen = await render(PageCard, {
    item: pageItem({ tab: true }, "Tab"),
    selected: false,
  });
  await expect.element(screen.getByText("Tab", { exact: true })).toBeVisible();
  await expect.element(screen.getByText("Sprint 42", { exact: true })).toBeVisible();
  await expect.element(screen.getByText("app.notion.com", { exact: true })).toBeVisible();
  expect(screen.container.querySelector(".frame img")).toBeNull();
  expect(screen.container.querySelector(".about .favicon")).not.toBeNull();
  await screen.unmount();
});

test("a request whose run used a signed-in session says so under its words", async () => {
  await page.viewport(1200, 800);
  const screen = await render(ObjectiveCard, {
    item: {
      id: "request",
      type: "request",
      kind: "Request",
      title: "What changed in the sprint?",
      detail: "",
      status: "",
      accounts: [{ host: "app.notion.com", used: 3, pages: 12 }],
    },
    selected: false,
    onaction: () => {},
  });
  await expect
    .element(
      screen.getByText("Using your session on app.notion.com · 3 of 12 pages", { exact: true }),
    )
    .toBeVisible();
  await expect
    .element(screen.getByRole("img", { name: "Read as you on app.notion.com" }))
    .toBeVisible();
  await screen.unmount();
});

function agentRun(status: WorkExecutionFact["status"]): WorkExecutionFact {
  const base = structuredClone(projection.executions[0]!);
  return {
    ...base,
    status,
    authorization: "user_directed_agent",
    artifacts: [],
    user_artifacts: [],
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
              accounts: [grant],
            },
          },
        },
      ],
    },
    steps: [],
    accounts: [{ host: "app.notion.com", pages_used: 12, pages: 12 }],
  };
}

test("a run that used every granted page says so plainly on the agent line", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = { ...structuredClone(projection), executions: [agentRun("failed")] };
  const screen = await render(AgentLine, { session });
  await expect
    .element(
      screen.getByText("The agent used all 12 pages it was allowed on app.notion.com", {
        exact: true,
      }),
    )
    .toBeVisible();
  await screen.unmount();
  session.dispose();
});

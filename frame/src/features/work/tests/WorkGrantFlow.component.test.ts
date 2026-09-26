import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { WorkEnvironmentSession } from "$domain/work-environment";
import type {
  WorkAccountGrantV1,
  WorkCallV1,
  WorkEnvironmentSnapshot,
  WorkOperationV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { tabFixture } from "$shared/testing/fixtures";
import WorkEnvironmentWorkspace from "../components/WorkEnvironmentWorkspace.svelte";

const native = vi.hoisted(() => ({ call: vi.fn(), operation: vi.fn(), status: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    faviconProbe: async () => true,
    workCall: native.call,
    workOperation: native.operation,
    workOperationStatus: native.status,
    workPaneHide: vi.fn(),
    workPaneSetRect: vi.fn(),
  });
});

const WORK = "01JWRK00000000000000000000";
const grant: WorkAccountGrantV1 = {
  origin: "https://app.slack.com",
  account: "01JACCOUNT0000000000000000",
  tab: "slack-tab",
  pages: 12,
};

/** A native Work that creates, attaches, opens and drafts a grant as Rust does. */
function nativeWork(profile: string, attached: boolean) {
  let revision = 1;
  const projection = (id = WORK): WorkRuntimeProjection => ({
    version: 1,
    interrupted: [],
    executions: [],
    work: {
      schema_version: 2,
      profile,
      id,
      revision: String(revision),
      lifecycle: "active",
      objective: "Summarise the channel",
      objective_revision: "1",
      context_revision: "1",
      objective_author: "user",
      questions: [],
      status: "draft",
      plan: null,
    },
  });
  const tab = { id: "tab-element", area: null, reference: { kind: "browser", tab: "slack-tab" } };
  const objective = {
    id: "goal-element",
    area: null,
    reference: { kind: "objective", objective: WORK },
  };
  let snapshot = {
    version: 1,
    profile,
    id: "environment",
    space: "space",
    title: "Slack",
    revision: "1",
    lifecycle: "active",
    areas: [],
    view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
    elements: attached ? [tab, objective] : [tab],
  } as WorkEnvironmentSnapshot;
  const operations: WorkOperationV1[] = [];
  native.call.mockImplementation(async (_profile: string, call: WorkCallV1) => {
    const reply = (value: object) => ({ version: 1, profile, reply: value });
    if (call.kind === "author") {
      revision += 1;
      return reply({
        kind: "authoring_applied",
        receipt: {
          command: call.command.command,
          work: WORK,
          applied_revision: String(revision),
          deleted: false,
        },
      });
    }
    if (call.kind === "query")
      return reply(
        call.request.query.kind === "list"
          ? { kind: "page", works: [], next: null }
          : { kind: "projection", projection: projection(call.request.query.work) },
      );
    if (call.kind === "environment") {
      const request = call.request;
      if (request.kind === "checkpoint") {
        snapshot = {
          ...snapshot,
          view: { ...request.view, revision: String(BigInt(request.expected) + 1n) },
        };
        return reply({
          kind: "environment",
          reply: {
            kind: "checkpointed",
            expected: request.expected,
            applied_view_revision: snapshot.view.revision,
            replayed: false,
            snapshot,
          },
        });
      }
      if (request.kind === "command") {
        if (request.intent.kind === "edit" && request.intent.edit.kind === "add")
          snapshot = {
            ...snapshot,
            revision: String(BigInt(snapshot.revision) + 1n),
            elements: [
              ...snapshot.elements,
              { id: "goal-element", area: null, reference: request.intent.edit.reference },
            ],
          };
        return reply({
          kind: "environment",
          reply: {
            kind: "applied",
            command: request.command,
            applied_revision: snapshot.revision,
            applied_view_revision: snapshot.view.revision,
            replayed: false,
            snapshot,
          },
        });
      }
      return reply({ kind: "environment", reply: { kind: "snapshot", snapshot } });
    }
    throw new Error(`unexpected ${call.kind}`);
  });
  const states = new Map<string, unknown>();
  native.operation.mockImplementation(
    async (_profile: string, operation: string, input: WorkOperationV1) => {
      operations.push(input);
      const state =
        input.kind === "prepare_account"
          ? {
              kind: "settled",
              response: {
                version: 1,
                profile,
                reply: { kind: "account_grant_draft", work: WORK, grant },
              },
            }
          : {
              kind: "settled",
              response: {
                version: 1,
                profile,
                reply: { kind: "projection", projection: projection() },
              },
            };
      // Rust admits the job and settles it later; the page learns it by polling.
      setTimeout(() => states.set(operation, state), 300);
      return { version: 1, profile, operation, state: { kind: "pending", work: WORK } };
    },
  );
  native.status.mockImplementation(async (_profile: string, _work: string, operation: string) => ({
    version: 1,
    profile,
    operation,
    state: states.get(operation) ?? { kind: "pending", work: WORK },
  }));
  return { snapshot, operations };
}

/** Picks the tab's site on its card, asks, and waits for the drafted grant's question. */
async function ask(profile: string, attached: boolean, earlier = false) {
  await page.viewport(1200, 800);
  const { snapshot, operations } = nativeWork(profile, attached);
  if (earlier) {
    // The profile's session was last on a work in another environment.
    const { workSession } = await import("$domain/work");
    const other = workSession(profile);
    other.selected = "01JWRKEARL1ER0000000000000";
    await other.start();
  }
  const environment = new WorkEnvironmentSession(profile, "space");
  environment.snapshot = snapshot;
  environment.selected = snapshot.id;
  environment.tabsIntroduced = true;
  const screen = await render(WorkEnvironmentWorkspace, {
    session: environment,
    tabs: [tabFixture({ id: "slack-tab", title: "Slack", url: "https://app.slack.com/client/T1" })],
    spaceName: "Personal",
    profileLabel: "Reader",
    aiEnabled: true,
    onreturn: vi.fn(),
    onopen: vi.fn(),
    onnewtab: vi.fn(),
    onsettings: vi.fn(),
  });
  const root = screen.container.querySelector(".environment") as HTMLElement;
  root.style.height = "720px";
  root.style.width = "1100px";
  const composer = screen.getByRole("textbox", {
    name: attached ? "Message the agent…" : "What do you want to do?",
    exact: true,
  });
  await expect.element(composer).toBeVisible();
  const tab = () =>
    screen.container.querySelector<HTMLElement>(
      '.svelte-flow__node[data-id="tab-element"] .work-drag-handle',
    );
  await expect.poll(tab).not.toBeNull();
  tab()!.click();
  await screen.getByRole("button", { name: "Ask signed in", exact: true }).click();
  await screen
    .getByRole("button", {
      name: "Let the agent read app.slack.com as me for this request",
      exact: true,
    })
    .click();
  await expect.poll(() => environment.accountScope?.mode).toBe("origin");
  await composer.fill("Summarise the channel");
  await screen.getByRole("button", { name: "Send", exact: true }).click();
  await expect.poll(() => operations.map((input) => input.kind)).toEqual(["prepare_account"]);
  const question = screen.getByRole("region", {
    name: "Let the agent read app.slack.com as me for this request",
  });
  await expect
    .element(question.getByText("Read app.slack.com as you for this request?", { exact: true }))
    .toBeVisible();
  await expect
    .element(question.getByText("up to 12 pages, reading only", { exact: true }))
    .toBeVisible();
  // The question stands in the agent line's place.
  expect(screen.container.querySelector(".agent-line")).toBeNull();
  return { screen, environment, operations };
}

async function allowed(profile: string, attached: boolean, earlier = false) {
  const { screen, environment, operations } = await ask(profile, attached, earlier);
  await screen.getByRole("button", { name: "Allow for this request", exact: true }).click();
  await expect
    .poll(() => operations.map((input) => input.kind))
    .toEqual(["prepare_account", "run"]);
  const run = operations[1]!;
  if (run.kind !== "run" || run.command.intent.kind !== "begin_agent") throw new Error("run");
  expect(run.command.intent.grant.accounts).toEqual([grant]);
  await expect.poll(() => screen.container.querySelector(".agent-line")).not.toBeNull();
  await screen.unmount();
  environment.dispose();
}

test("the first signed-in request asks for its grant, and allowing it runs", async () => {
  await allowed("grant-first", false);
});

test("a signed-in follow-up asks for its grant, and allowing it runs", async () => {
  await allowed("grant-follow", true);
});

test("a first request after another environment's work asks for its grant", async () => {
  await allowed("grant-earlier", false, true);
});

test("not now continues the request without the session", async () => {
  const { screen, environment, operations } = await ask("grant-decline", false);
  await screen.getByRole("button", { name: "Not now", exact: true }).click();
  await expect
    .element(screen.getByText("Continuing without your session", { exact: true }))
    .toBeVisible();
  await expect
    .poll(() => operations.map((input) => input.kind))
    .toEqual(["prepare_account", "run"]);
  const run = operations[1]!;
  if (run.kind !== "run" || run.command.intent.kind !== "begin_agent") throw new Error("run");
  expect(run.command.intent.grant.accounts).toBeUndefined();
  await screen.unmount();
  environment.dispose();
});

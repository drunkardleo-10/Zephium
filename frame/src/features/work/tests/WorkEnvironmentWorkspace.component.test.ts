import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { WorkEnvironmentSession } from "$domain/work-environment";
import type { WorkCallV1, WorkEnvironmentSnapshot } from "$shared/ipc/bindings";
import { tabFixture } from "$shared/testing/fixtures";
import WorkEnvironmentWorkspace from "../components/WorkEnvironmentWorkspace.svelte";
const native = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call });
});

test("the production manual environment attaches a real tab and opens only its explicit Browse action", async () => {
  const profile = "00000000000000000000000001";
  const space = "00000000000000000000000002";
  const id = "00000000000000000000000003";
  let snapshot: WorkEnvironmentSnapshot = {
    version: 1,
    id,
    profile,
    space,
    title: "Manual research",
    lifecycle: "active",
    revision: "1",
    elements: [],
    areas: [],
    view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
  };
  native.call.mockImplementation(async (_profile: string, call: WorkCallV1) => {
    if (call.kind !== "environment")
      throw new Error("Manual Work must not initialize objective execution");
    const request = call.request;
    if (request.kind === "checkpoint") {
      snapshot = {
        ...snapshot,
        view: { ...request.view, revision: String(BigInt(request.expected) + 1n) },
      };
      return {
        version: 1,
        profile,
        reply: {
          kind: "environment",
          reply: {
            kind: "checkpointed",
            expected: request.expected,
            applied_view_revision: snapshot.view.revision,
            replayed: false,
            snapshot,
          },
        },
      };
    }
    if (request.kind === "command") {
      if (request.intent.kind === "edit" && request.intent.edit.kind === "add") {
        snapshot = {
          ...snapshot,
          revision: "2",
          elements: [
            {
              id: "00000000000000000000000004",
              area: null,
              reference: request.intent.edit.reference,
            },
          ],
        };
      }
      return {
        version: 1,
        profile,
        reply: {
          kind: "environment",
          reply: {
            kind: "applied",
            command: request.command,
            applied_revision: snapshot.revision,
            applied_view_revision: snapshot.view.revision,
            replayed: false,
            snapshot,
          },
        },
      };
    }
    return {
      version: 1,
      profile,
      reply: { kind: "environment", reply: { kind: "snapshot", snapshot } },
    };
  });
  const session = new WorkEnvironmentSession(profile, space);
  session.snapshot = snapshot;
  session.selected = id;
  const onopen = vi.fn();
  const screen = await render(WorkEnvironmentWorkspace, {
    session,
    tabs: [tabFixture({ id: "retained-tab", title: "Research tab" })],
    spaceName: "Personal",
    profileLabel: "Reader",
    aiEnabled: false,
    onreturn: vi.fn(),
    onopen,
    onnewtab: vi.fn(),
    onsettings: vi.fn(),
  });
  const root = screen.container.querySelector(".environment") as HTMLElement;
  root.style.height = "720px";
  root.style.width = "1100px";
  await screen.getByRole("checkbox", { name: /Research tab/ }).click();
  await screen.getByRole("button", { name: "Add to Work (1)" }).click();
  await expect
    .poll(() => session.snapshot?.elements[0]?.reference)
    .toEqual({ kind: "browser", tab: "retained-tab" });
  expect(onopen).not.toHaveBeenCalled();
  await screen.getByRole("button", { name: "Tabs", exact: true }).click();
  await screen.getByRole("button", { name: "Inspect Research tab" }).click();
  await screen.getByRole("button", { name: "Open in Browse", exact: true }).click();
  expect(onopen).toHaveBeenCalledExactlyOnceWith("retained-tab");
  await expect
    .element(screen.getByRole("textbox", { name: "Start a new objective" }))
    .not.toBeInTheDocument();
  await screen.unmount();
  session.dispose();
});

test("opening an objective replaces selection controls and focuses its visible inspector", async () => {
  const { workSession } = await import("$domain/work");
  const { projection, snapshot } = await import("./environment-fixtures");
  const environment = new WorkEnvironmentSession(snapshot.profile, snapshot.space);
  environment.snapshot = structuredClone(snapshot);
  environment.selected = snapshot.id;
  environment.tabsIntroduced = true;
  const objective = workSession(snapshot.profile)!;
  vi.spyOn(objective, "start").mockResolvedValue();
  vi.spyOn(objective, "open").mockImplementation(async () => {
    objective.projection = structuredClone(projection);
    return true;
  });
  vi.spyOn(objective, "plan").mockResolvedValue(null);
  native.call.mockResolvedValue({
    version: 1,
    profile: snapshot.profile,
    reply: { kind: "error", error: "not_found" },
  });
  const screen = await render(WorkEnvironmentWorkspace, {
    session: environment,
    tabs: [],
    spaceName: "Personal",
    profileLabel: "Reader",
    aiEnabled: false,
    onreturn: vi.fn(),
    onopen: vi.fn(),
    onnewtab: vi.fn(),
    onsettings: vi.fn(),
  });
  const root = screen.container.querySelector(".environment") as HTMLElement;
  root.style.height = "720px";
  root.style.width = "1100px";
  await screen.getByRole("button", { name: "Inspect Objective", exact: true }).click();
  await screen.getByRole("button", { name: "Open resource", exact: true }).click();
  const panel = screen.getByRole("region", { name: "Objective", exact: true });
  await expect.element(panel).toBeVisible();
  await expect.element(panel).toHaveFocus();
  expect(screen.container.querySelector(".inspector")).toBeNull();
  await screen.getByRole("button", { name: "Back to canvas", exact: true }).click();
  expect(screen.container.querySelector(".inspector")).toBeNull();
  await screen.unmount();
  environment.dispose();
  objective.dispose();
});

test("a real attached objective expands its historical responsibilities directly on canvas", async () => {
  const { projection, snapshot } = await import("./environment-fixtures");
  const state = { ...projection, work: { ...projection.work, profile: "graph-profile" } };
  const environment = new WorkEnvironmentSession("graph-profile", snapshot.space);
  environment.snapshot = { ...structuredClone(snapshot), profile: "graph-profile" };
  environment.selected = snapshot.id;
  environment.tabsIntroduced = true;
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind === "query")
      return {
        version: 1,
        profile: "graph-profile",
        reply:
          call.request.query.kind === "plan"
            ? {
                kind: "plan",
                plan: {
                  author: "user",
                  revision: "2",
                  basis_revision: "1",
                  draft: {
                    id: "plan",
                    nodes: [
                      {
                        id: "read",
                        objective: "Read evidence",
                        dependencies: [],
                        outputs: [
                          {
                            name: "Evidence shortlist",
                            description: "Operational prose should stay in optional details",
                            review: "user_acceptance",
                          },
                        ],
                      },
                      {
                        id: "compare",
                        objective: "Compare findings",
                        dependencies: ["read"],
                        outputs: [],
                      },
                    ],
                  },
                },
              }
            : { kind: "projection", projection: state },
      };
    if (call.kind !== "environment" || call.request.kind !== "checkpoint")
      throw new Error("Unexpected mutation");
    expect(call.request.view.placements.map((place) => place.element)).toEqual([
      "objective-card",
      "result-card",
    ]);
    const request = call.request;
    return {
      version: 1,
      profile: "graph-profile",
      reply: {
        kind: "environment",
        reply: {
          kind: "checkpointed",
          expected: request.expected,
          applied_view_revision: String(BigInt(request.expected) + 1n),
          replayed: false,
          snapshot: {
            ...environment.snapshot!,
            view: { ...request.view, revision: String(BigInt(request.expected) + 1n) },
          },
        },
      },
    };
  });
  const screen = await render(WorkEnvironmentWorkspace, {
    session: environment,
    tabs: [],
    spaceName: "Personal",
    profileLabel: "Reader",
    aiEnabled: false,
    onreturn: vi.fn(),
    onopen: vi.fn(),
    onnewtab: vi.fn(),
    onsettings: vi.fn(),
  });
  const root = screen.container.querySelector(".environment") as HTMLElement;
  root.style.height = "720px";
  root.style.width = "1400px";
  await screen.getByRole("button", { name: "Show plan", exact: true }).click();
  await expect
    .element(screen.getByRole("button", { name: "Inspect Read evidence", exact: true }))
    .toBeVisible();
  await expect
    .element(screen.getByRole("button", { name: "Inspect Compare findings", exact: true }))
    .toBeVisible();
  await expect.element(screen.getByText("Evidence shortlist", { exact: true })).toBeVisible();
  expect(screen.container.querySelectorAll("article.responsibility")).toHaveLength(2);
  expect(screen.container.textContent).not.toContain(
    "Operational prose should stay in optional details",
  );
  await expect.poll(() => screen.container.querySelectorAll(".svelte-flow__edge").length).toBe(2);
  const dependency = screen.container.querySelector(
    '[aria-label="Compare findings depends on Read evidence"]',
  );
  expect(dependency).not.toBeNull();
  expect(dependency?.getAttribute("tabindex")).toBeNull();
  expect(dependency?.getAttribute("aria-describedby")).toBeNull();
  await screen.getByRole("button", { name: "Collapse plan", exact: true }).click();
  await expect
    .element(screen.getByRole("button", { name: "Inspect Read evidence", exact: true }))
    .not.toBeInTheDocument();
  await screen.getByRole("button", { name: "Show plan", exact: true }).click();
  await expect
    .element(screen.getByRole("button", { name: "Inspect Read evidence", exact: true }))
    .toBeVisible();
  await environment.flushView();
  await screen.unmount();
  environment.dispose();
});

test("prompt submission keeps work on canvas and clarification choices above the available composer", async () => {
  await page.viewport(1100, 750);
  const { workSession } = await import("$domain/work");
  const { projection, snapshot } = await import("./environment-fixtures");
  const environment = new WorkEnvironmentSession("interaction-profile", "space");
  environment.snapshot = { ...snapshot, profile: "interaction-profile", elements: [] };
  environment.tabsIntroduced = true;
  const objective = workSession("interaction-profile")!;
  vi.spyOn(objective, "start").mockResolvedValue();
  vi.spyOn(objective, "open").mockResolvedValue(true);
  const create = vi.spyOn(objective, "create").mockImplementation(async (text) => {
    objective.selected = "objective";
    objective.projection = {
      ...projection,
      executions: [],
      work: {
        ...projection.work,
        profile: "interaction-profile",
        status: "draft",
        objective: text,
      },
    };
    return true;
  });
  const planning = vi.spyOn(objective.operations, "begin").mockResolvedValue();
  vi.spyOn(environment, "edit").mockImplementation(async (edit) => {
    expect(edit).toMatchObject({
      kind: "add",
      reference: { kind: "objective", objective: "objective" },
    });
    environment.snapshot = { ...environment.snapshot!, elements: [snapshot.elements[0]!] };
    return true;
  });
  native.call.mockResolvedValue({
    version: 1,
    profile: "interaction-profile",
    reply: { kind: "error", error: "not_found" },
  });
  const screen = await render(WorkEnvironmentWorkspace, {
    session: environment,
    tabs: [],
    spaceName: "Personal",
    profileLabel: "Reader",
    aiEnabled: true,
    onreturn: vi.fn(),
    onopen: vi.fn(),
    onnewtab: vi.fn(),
    onsettings: vi.fn(),
  });
  const root = screen.container.querySelector(".environment") as HTMLElement;
  root.style.height = "600px";
  root.style.width = "1100px";
  const composer = screen.getByRole("textbox", {
    name: "Start a new objective",
    exact: true,
  });
  await expect.element(screen.getByRole("button", { name: "Fit view", exact: true })).toBeVisible();
  const longObjective =
    "Find a keyboard with quiet switches and compare available options, delivery dates, prices, and compatibility for my workspace.";
  await composer.fill(longObjective);
  await screen.getByRole("button", { name: "Start new work", exact: true }).click();
  await expect.poll(() => create.mock.calls).toEqual([[longObjective, expect.any(String)]]);
  await expect
    .poll(() => planning.mock.calls)
    .toEqual([
      [
        {
          kind: "plan",
          request: { version: 1, work: "objective", expected_revision: "4" },
        },
      ],
    ]);
  expect(screen.container.querySelector(".detail")).toBeNull();
  expect(screen.container.querySelector(".approval")).toBeNull();
  objective.projection = {
    ...objective.projection!,
    work: {
      ...objective.projection!.work,
      status: "needs_input",
      questions: [
        {
          id: "budget",
          basis_revision: "4",
          objective_revision: "1",
          state: "active",
          author: "primary_agent",
          answer_author: null,
          prompt: "Choose a budget",
          options: ["Under 150", "Under 300"],
          answer: null,
        },
      ],
    },
  };
  await screen.getByRole("button", { name: "Under 150", exact: true }).click();
  expect(objective.draft("question:budget")).toBe("Under 150");
  await expect.element(composer).toBeVisible();
  const question = screen.container.querySelector(".interaction input")!;
  const prompt = screen.container.querySelector(".input-panel textarea")!;
  expect(question.compareDocumentPosition(prompt) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(question.getBoundingClientRect().bottom).toBeLessThan(prompt.getBoundingClientRect().top);
  const save = vi.spyOn(objective, "saveDraft").mockResolvedValue(false);
  await screen.getByRole("button", { name: "Continue", exact: true }).click();
  expect(save).toHaveBeenCalledExactlyOnceWith("question:budget", "Under 150");
  expect(planning).toHaveBeenCalledTimes(1);
  expect(objective.draft("question:budget")).toBe("Under 150");
  const activeHeader = screen.container.querySelector(".interaction header")!;
  expect(activeHeader.querySelector("button")!.getBoundingClientRect().right).toBeLessThanOrEqual(
    activeHeader.getBoundingClientRect().right,
  );
  const link = { extraction_id: "provider-record", source_id: 1 };
  const completed = structuredClone(projection.executions[0]!);
  completed.user_artifacts![0]!.evidence = [link];
  completed.spec.nodes.push({ ...completed.spec.nodes[0]!, node: "child-node", parent: "node" });
  completed.artifacts.push({
    ...completed.artifacts[0]!,
    id: "supporting-result",
    node: "child-node",
    title: "Supporting research",
  });
  objective.projection = {
    ...objective.projection!,
    work: { ...objective.projection!.work, questions: [], status: "plan_ready" },
    executions: [completed],
  };
  root.style.height = "750px";
  const readEvidence = vi.spyOn(objective, "evidence").mockResolvedValue({
    version: 1,
    link,
    origin: "example.com",
    role: "citation",
    text: "Retained source",
    truncated: false,
    source_bytes: "15",
    source: {
      kind: "provider_search",
      provider: "open_ai",
      model: "search-model",
      url: "https://example.com/source",
      title: "Exact cited source",
      response_id: "response",
      search_call_id: "search-call",
    },
  });
  await screen.getByRole("button", { name: "Fit view", exact: true }).click();
  await expect.element(screen.getByText("Reviewed findings", { exact: true })).toBeVisible();
  expect(screen.container.querySelector(".detail")).toBeNull();
  await screen.getByRole("button", { name: "Source 1", exact: true }).click();
  await expect.element(screen.getByText("Exact cited source", { exact: true })).toBeVisible();
  expect(readEvidence).toHaveBeenCalledExactlyOnceWith(link);
  await expect
    .element(screen.getByRole("button", { name: "Edit result", exact: true }))
    .toBeVisible();
  expect(screen.container.querySelector(".detail")).toBeNull();
  await screen.getByRole("button", { name: "Close", exact: true }).click();
  objective.discardDrafts();
  await expect
    .poll(() => screen.container.querySelector(".interaction.settled") !== null)
    .toBe(true);
  await expect
    .poll(
      () => screen.container.querySelector(".interaction-stack")!.getBoundingClientRect().height,
    )
    .toBeLessThan(110);
  const details = screen.getByRole("button", { name: "Plan details", exact: true });
  await expect.element(details).toBeVisible();
  const header = screen.container.querySelector(".interaction header")!;
  const title = header.querySelector("strong")!;
  const button = header.querySelector("button")!;
  expect(title.getBoundingClientRect().right).toBeLessThan(button.getBoundingClientRect().left);
  expect(button.getBoundingClientRect().right).toBeLessThanOrEqual(
    header.getBoundingClientRect().right,
  );
  expect(button.getBoundingClientRect().right).toBeLessThanOrEqual(1100);
  await screen.getByRole("navigation").getByRole("button", { name: "Create", exact: true }).click();
  await screen.getByRole("button", { name: "List view", exact: true }).click();
  await screen
    .getByRole("navigation")
    .getByRole("button", { name: "Create", exact: true, expanded: true })
    .click();
  await expect
    .poll(() => screen.container.querySelector(".list-view")?.textContent)
    .toContain("Reviewed findings");
  await expect.element(screen.getByRole("button", { name: "Source 1", exact: true })).toBeEnabled();
  await screen.getByRole("button", { name: "View 1 other results", exact: true }).click();
  await expect
    .element(screen.getByRole("button", { name: "Supporting research", exact: true }))
    .toBeVisible();

  await expect
    .element(screen.getByRole("button", { name: "Back to canvas", exact: true }))
    .toBeVisible();
  await screen.unmount();
  environment.dispose();
  objective.dispose();
});

test("public research is explicit, validates without truncation, and submits only the saved objective", async () => {
  await page.viewport(1100, 750);
  const { workSession } = await import("$domain/work");
  const { projection, snapshot } = await import("./environment-fixtures");
  const profile = "public-composer-profile";
  const environment = new WorkEnvironmentSession(profile, "space");
  environment.snapshot = { ...snapshot, profile, elements: [] };
  environment.tabsIntroduced = true;
  const objective = workSession(profile)!;
  vi.spyOn(objective, "start").mockResolvedValue();
  vi.spyOn(objective, "open").mockResolvedValue(true);
  const create = vi.spyOn(objective, "create").mockImplementation(async (text) => {
    objective.selected = "objective";
    objective.projection = {
      ...projection,
      executions: [],
      work: { ...projection.work, profile, status: "draft", objective: text },
    };
    return true;
  });
  const operation = vi.spyOn(objective.operations, "begin").mockResolvedValue();
  vi.spyOn(environment, "edit").mockResolvedValue(true);
  native.call.mockResolvedValue({
    version: 1,
    profile,
    reply: { kind: "error", error: "not_found" },
  });
  const screen = await render(WorkEnvironmentWorkspace, {
    session: environment,
    tabs: [],
    spaceName: "Personal",
    profileLabel: "Reader",
    onreturn: vi.fn(),
    onopen: vi.fn(),
    onnewtab: vi.fn(),
    onsettings: vi.fn(),
  });
  const root = screen.container.querySelector<HTMLElement>(".environment")!;
  root.style.height = "600px";
  root.style.width = "1100px";
  const composer = screen.getByRole("textbox", { name: "Start a new objective", exact: true });
  await composer.fill("🔬".repeat(513));
  await screen.getByRole("checkbox", { name: "Research public web", exact: true }).click();
  await expect
    .element(
      screen.getByText(
        "OpenAI · GPT-5.6 Luna. Sends only this objective to public web search; excludes attached resources, private and account context. Maximum $0.10.",
        { exact: true },
      ),
    )
    .toBeVisible();
  await screen.getByRole("button", { name: "Research public web", exact: true }).click();
  await expect.element(screen.getByRole("alert")).toBeVisible();
  expect(create).not.toHaveBeenCalled();
  expect(environment.composer).toBe("🔬".repeat(513));
  await composer.fill("  Find cafés in Łódź 🔬  ");
  await screen.getByRole("button", { name: "Research public web", exact: true }).click();
  await expect.poll(() => operation.mock.calls.length).toBe(1);
  expect(create).toHaveBeenCalledExactlyOnceWith("Find cafés in Łódź 🔬", expect.any(String));
  expect(operation.mock.calls[0]![0]).toEqual({
    kind: "read_public",
    command: {
      version: 1,
      work: "objective",
      expected_revision: "4",
      command: expect.any(String),
      intent: {
        kind: "read_public",
        scope: { provider: "open_ai", model: "gpt-5.6-luna", query: "Find cafés in Łódź 🔬" },
        limits: {
          model_tokens: 147456,
          cost_micro_usd: 100000,
          operations: 1,
          timeout_seconds: 180,
          max_workers: 1,
        },
      },
    },
  });
  expect(environment.composer).toBe("");
  await expect
    .element(screen.getByRole("button", { name: "Back to canvas", exact: true }))
    .not.toBeInTheDocument();
  await screen.unmount();
  environment.dispose();
});

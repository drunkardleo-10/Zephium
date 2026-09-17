import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { WorkEnvironmentSession } from "$domain/work-environment";
import type { WorkEnvironmentSnapshot } from "$shared/ipc/bindings";
import { tabFixture } from "$shared/testing/fixtures";
import WorkEnvironmentWorkspace from "../components/WorkEnvironmentWorkspace.svelte";
const native = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call });
});

test("a tab card starts a signed-in request that Rust prepares for approval", async () => {
  await page.viewport(1200, 800);
  const { workSession } = await import("$domain/work");
  const { projection } = await import("./environment-fixtures");
  const profile = "00000000000000000000000001";
  const snapshot: WorkEnvironmentSnapshot = {
    version: 1,
    id: "00000000000000000000000003",
    profile,
    space: "00000000000000000000000002",
    title: "Sprint",
    lifecycle: "active",
    revision: "1",
    elements: [
      {
        id: "00000000000000000000000004",
        area: null,
        reference: { kind: "browser", tab: "tab-1" },
      },
    ],
    areas: [],
    view: {
      revision: "1",
      x: 0,
      y: 0,
      zoom_milli: 1000,
      placements: [
        { element: "00000000000000000000000004", x: 80, y: 80, width: 320, height: 200 },
      ],
    },
  };
  native.call.mockResolvedValue({
    version: 1,
    profile,
    reply: { kind: "environment", reply: { kind: "snapshot", snapshot } },
  });
  const environment = new WorkEnvironmentSession(profile, snapshot.space);
  environment.snapshot = snapshot;
  environment.selected = snapshot.id;
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
  const screen = await render(WorkEnvironmentWorkspace, {
    session: environment,
    tabs: [
      tabFixture({ id: "tab-1", title: "Sprint 42", url: "https://app.notion.com/p/Sprint-42" }),
    ],
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
  await expect.poll(() => screen.container.querySelectorAll(".work-drag-handle").length).toBe(1);
  await screen.container.querySelector<HTMLElement>(".work-drag-handle")!.click();
  await screen.getByRole("button", { name: "Ask signed in", exact: true }).click();
  await expect.element(screen.getByRole("group", { name: "Signed-in page" })).toBeVisible();
  await expect
    .element(screen.getByText("https://app.notion.com", { exact: true }).first())
    .toBeVisible();
  const composer = screen.getByRole("textbox", { name: "What do you want to do?", exact: true });
  await expect.element(composer).toHaveFocus();
  await screen.getByRole("radio", { name: "Change a field and restore it", exact: true }).click();
  await composer.fill("Rename the sprint page briefly");
  await screen.getByRole("button", { name: "Send", exact: true }).click();
  await expect.element(screen.getByRole("alert")).toBeVisible();
  expect(create).not.toHaveBeenCalled();
  await screen.getByRole("textbox", { name: "Current value", exact: true }).fill("Sprint 42");
  await screen
    .getByRole("textbox", { name: "Temporary value", exact: true })
    .fill("Sprint 42 (probe)");
  await screen.getByRole("button", { name: "Send", exact: true }).click();
  await expect.poll(() => operation.mock.calls.length).toBe(1);
  expect(create).toHaveBeenCalledExactlyOnceWith(
    "Rename the sprint page briefly",
    expect.any(String),
  );
  expect(operation.mock.calls[0]![0]).toEqual({
    kind: "prepare_account",
    request: {
      version: 1,
      work: "objective",
      expected_revision: "4",
      environment: snapshot.id,
      element: "00000000000000000000000004",
      effect: {
        kind: "update",
        update: { field: null, from: "Sprint 42", to: "Sprint 42 (probe)" },
      },
    },
  });
  expect(environment.accountScope).toBeNull();
  expect(environment.composer).toBe("");
  await screen.unmount();
  environment.dispose();
});

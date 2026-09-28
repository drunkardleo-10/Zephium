import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import { WorkEnvironmentSession } from "$domain/work-environment";
import WorkEnvironmentWorkspace from "../components/WorkEnvironmentWorkspace.svelte";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  const { models } = await import("$shared/testing/work-models");
  return mockBindings({
    workModels: vi.fn(async (profile: string) => ({
      ...models({ keys: { anthropic: "valid" }, lead: "anthropic/claude-opus-5-5" }),
      profile,
    })),
    faviconProbe: async () => true,
    runCommand: vi.fn(async () => ({ accepted: true, operation_id: null })),
    workCall: vi.fn(async (profile: string) => ({
      version: 1,
      profile,
      reply: { kind: "error" as const, error: "not_found" as const },
    })),
  });
});

const shots = "../../../../../target/work-shell";
const settle = () => new Promise((done) => setTimeout(done, 450));

test("the Work screen's own chrome, at rest and in use, in both themes", async () => {
  const { workSession } = await import("$domain/work");
  const { projection, snapshot } = await import("./environment-fixtures");
  await page.viewport(1440, 900);
  const environment = new WorkEnvironmentSession(snapshot.profile, snapshot.space);
  environment.snapshot = { ...structuredClone(snapshot), title: "YC trip from Warsaw" };
  environment.selected = snapshot.id;
  environment.works = [
    ["YC trip from Warsaw", snapshot.id],
    ["AI SaaS architecture and budget", "a"],
    ["SQLite, DuckDB or RocksDB", "b"],
    ["New work", "c"],
    ["Old launch plan", "d"],
  ].map(([title, id]) => ({
    id: id!,
    space: snapshot.space,
    title: title!,
    lifecycle: id === "d" ? ("archived" as const) : ("active" as const),
    revision: "1",
  }));
  const objective = workSession(snapshot.profile)!;
  vi.spyOn(objective, "start").mockResolvedValue();
  vi.spyOn(objective, "open").mockImplementation(async () => {
    objective.projection = structuredClone(projection);
    return true;
  });
  vi.spyOn(objective, "plan").mockResolvedValue(null);
  const screen = await render(WorkEnvironmentWorkspace, {
    session: environment,
    tabs: [],
    spaceName: "Personal",
    profileLabel: "Reader",
    aiEnabled: true,
    onopen: vi.fn(),
    onnewtab: vi.fn(),
  });
  const root = screen.container.querySelector(".environment") as HTMLElement;
  root.style.height = "884px";
  root.style.width = "1376px";
  const shoot = async (name: string) => {
    for (const theme of ["dark", "light"]) {
      document.documentElement.dataset.theme = theme;
      await settle();
      await page.screenshot({ path: `${shots}/${name}-${theme}.png` });
    }
    document.documentElement.dataset.theme = "dark";
  };
  await shoot("rest");
  await screen.getByRole("button", { name: "Select", exact: true }).hover();
  await new Promise((done) => setTimeout(done, 900));
  await shoot("hint");
  await screen.getByRole("button", { name: "Works" }).hover();
  const field = screen.getByRole("textbox", { name: "What do you want to do?" });
  await field.click();
  await userEvent.keyboard("Plan my trip to the next YC batch from Warsaw");
  await expect
    .poll(() => screen.container.querySelector<HTMLButtonElement>(".model-trigger")?.disabled)
    .toBe(false);
  await shoot("typing");
  await screen.getByRole("button", { name: "Private run", exact: true }).click();
  await shoot("private");
  await field.fill("");
  (document.activeElement as HTMLElement | null)?.blur();
  await screen.getByRole("button", { name: "Works" }).click();
  await expect.element(screen.getByRole("searchbox", { name: "Search works" })).toBeVisible();
  await shoot("works");
  await userEvent.keyboard("{Escape}");
  await screen.getByRole("button", { name: "Account" }).click();
  await shoot("account");
  await userEvent.keyboard("{Escape}");
  await screen.getByRole("button", { name: "Note", exact: true }).click();
  await expect.element(screen.getByRole("searchbox", { name: "Search notes" })).toBeVisible();
  await shoot("note");
  await userEvent.keyboard("{Escape}");
  const live = structuredClone(projection);
  const execution = live.executions[0]!;
  execution.status = "running";
  execution.authorization = "user_directed_agent";
  execution.attempts = [{ id: "attempt", node: "node", status: "running", usage: null }];
  execution.steps = [
    {
      id: "read-1",
      turn: 1,
      kind: { kind: "read", url: "https://www.airbnb.com/s/San-Francisco" },
      status: "running",
    },
  ];
  objective.selected = "objective";
  objective.projection = live;
  await expect.poll(() => screen.container.querySelector(".island")).not.toBeNull();
  await shoot("island");
  await screen.unmount();
  environment.dispose();
});

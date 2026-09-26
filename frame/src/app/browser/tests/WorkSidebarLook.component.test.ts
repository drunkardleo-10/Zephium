import { expect, test, vi } from "vitest";
import { page } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import { surface } from "$domain/surface";
import { environmentSession } from "$domain/work-environment";
import { emitNativeEvent } from "$shared/testing/native-events";
import WorkSidebarLook from "./WorkSidebarLook.svelte";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    runCommand: vi.fn(async () => ({ accepted: true, operation_id: null })),
  });
});

const shots = "../../../../../target/work-shell";

async function projects() {
  await surface.init();
  emitNativeEvent("uiCommand", "browser.work");
  const work = environmentSession("profile", "space")!;
  work.works = [
    ["saas", "SaaS design"],
    ["trip", "YC trip"],
    ["jobs", "Job finding"],
    ["agents", "AI agents research"],
    ["old", "Old launch plan"],
  ].map(([id, title]) => ({
    id: id!,
    space: "space",
    title: title!,
    lifecycle: id === "old" ? ("archived" as const) : ("active" as const),
    revision: "1",
  }));
  work.selected = "trip";
  work.running = "trip";
}

test("the sidebar in Work lists projects as rows", async () => {
  await projects();
  const screen = await render(WorkSidebarLook);
  await page.viewport(420, 700);
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((settled) => setTimeout(settled, 400));
    await page
      .elementLocator(screen.container)
      .screenshot({ path: `${shots}/sidebar-${theme}.png` });
  }
  await expect.element(screen.getByRole("button", { name: "YC trip, working" })).toBeVisible();
});

test("the rail in Work shows projects the way it shows tabs", async () => {
  await projects();
  const screen = await render(WorkSidebarLook, { props: { compact: true } });
  await page.viewport(420, 700);
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((settled) => setTimeout(settled, 400));
    await page.elementLocator(screen.container).screenshot({ path: `${shots}/rail-${theme}.png` });
  }
  await expect.element(screen.getByRole("button", { name: "New project" })).toBeVisible();
});

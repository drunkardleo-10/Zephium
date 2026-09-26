import { expect, test, vi } from "vitest";
import { page, userEvent } from "vitest/browser";
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

test("a project row's context menu renames it in place and archives it", async () => {
  await projects();
  const work = environmentSession("profile", "space")!;
  const edit = vi.spyOn(work, "editWork").mockResolvedValue(true);
  const open = vi.spyOn(work, "open").mockResolvedValue(true);
  const screen = await render(WorkSidebarLook);
  const row = screen.getByRole("button", { name: "Job finding", exact: true });
  await expect.element(row).toBeVisible();
  row
    .element()
    .dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 60, clientY: 90 }),
    );
  await page.getByRole("menuitem", { name: "Rename" }).click();
  const field = screen.getByRole("textbox", { name: "Project title" });
  await expect.element(field).toHaveFocus();
  await field.fill("Job hunt");
  await userEvent.keyboard("{Enter}");
  expect(edit).toHaveBeenCalledWith("jobs", { kind: "rename", title: "Job hunt" });
  // Renaming another project leaves the canvas on the one that was open.
  await expect.poll(() => open.mock.lastCall).toEqual(["trip"]);

  row
    .element()
    .dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 60, clientY: 90 }),
    );
  await page.getByRole("menuitem", { name: "Archive" }).click();
  await expect
    .poll(() => edit.mock.lastCall)
    .toEqual(["jobs", { kind: "set_lifecycle", lifecycle: "archived" }]);
  // Archiving a project that is not the open one leaves the canvas where it is.
  await expect.poll(() => open.mock.lastCall).toEqual(["trip"]);
  edit.mockRestore();
  open.mockRestore();
});

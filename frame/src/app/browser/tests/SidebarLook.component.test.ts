import { expect, test, vi } from "vitest";
import { page } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import SidebarLook from "./SidebarLook.svelte";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    runCommand: vi.fn(async () => ({ accepted: true, operation_id: null })),
    sidebarSetWidth: vi.fn(async () => undefined),
  });
});

// A rendered reference for the column's resting shape, in both themes and
// both widths. Static checks cannot tell whether the base of the sidebar
// reads as one band.
test("the sidebar column at rest", async () => {
  const screen = await render(SidebarLook);
  await page.viewport(420, 700);
  document.documentElement.dataset.theme = "dark";
  await page
    .elementLocator(screen.container)
    .screenshot({ path: "../../../../../target/sidebar-dark.png" });
  document.documentElement.dataset.theme = "light";
  await page
    .elementLocator(screen.container)
    .screenshot({ path: "../../../../../target/sidebar-light.png" });
  // Kept sites divide the row evenly; one alone would fill it.
  const row = screen.container.querySelector<HTMLElement>(".dock-row")!;
  expect(getComputedStyle(row).gridTemplateColumns.split(" ")).toHaveLength(3);

  // And the shelf revealed, which is the only state the column grows into.
  document.documentElement.dataset.theme = "dark";
  await screen.getByRole("button", { name: "Tools", exact: true }).hover();
  const flyout = screen.container.querySelector<HTMLElement>(".flyout")!;
  await expect.poll(() => flyout.inert).toBe(false);
  // The stack staggers in; capture it settled, not mid-rise.
  const nearest = flyout.querySelector<HTMLElement>(".tool:last-child")!;
  await expect.poll(() => getComputedStyle(nearest).opacity).toBe("1");
  await page
    .elementLocator(screen.container)
    .screenshot({ path: "../../../../../target/sidebar-shelf.png" });
});

test("the rail at rest", async () => {
  const screen = await render(SidebarLook, { props: { compact: true } });
  await page.viewport(420, 700);
  document.documentElement.dataset.theme = "dark";
  await page
    .elementLocator(screen.container)
    .screenshot({ path: "../../../../../target/rail-dark.png" });

  // The rail's stack is glyphs only: at 56px a name has nowhere to go.
  await screen.getByRole("button", { name: "Tools", exact: true }).hover();
  const flyout = screen.container.querySelector<HTMLElement>(".flyout")!;
  await expect.poll(() => flyout.inert).toBe(false);
  const nearest = flyout.querySelector<HTMLElement>(".tool:last-child")!;
  await expect.poll(() => getComputedStyle(nearest).opacity).toBe("1");
  expect(nearest.textContent?.trim()).toBe("");
  await page
    .elementLocator(screen.container)
    .screenshot({ path: "../../../../../target/rail-shelf.png" });
});

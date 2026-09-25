import { expect, test, vi } from "vitest";
import { page, userEvent } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import { resourceTestServer } from "$shared/testing/resources/server";
import TaskHost from "./TaskHost.svelte";
import PageHost from "./PageHost.svelte";
import { setPageView } from "../lib/page-view.svelte";

const native = vi.hoisted(() => ({
  call: vi.fn(),
  open: vi.fn(),
  profile: "00000000000000000000000095",
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ resourceCall: native.call, browserOpenUrl: native.open });
});
vi.mock("$domain/tabs", () => ({
  tabs: { profile: () => ({ id: native.profile }), activeTab: () => null },
}));
vi.mock("$domain/surface", () => ({ surface: { open: vi.fn() } }));

async function add(within: Awaited<ReturnType<typeof render>>, title: string) {
  const field = within.getByRole("textbox", { name: "New task", exact: true });
  await field.click();
  await field.fill(title);
  await userEvent.keyboard("{Enter}");
}

/** A session outlives the view that first built it: the panel, then the full
 *  page, then the panel again must all keep drawing what native settles. */
test("the panel keeps updating after a trip through the full page", async () => {
  await page.viewport(1200, 800);
  const server = resourceTestServer(native.profile);
  native.call.mockImplementation(server.call);

  const panel = await render(TaskHost, { profile: native.profile, scope: "all" });
  await add(panel, "First");
  await expect.element(panel.getByText("First", { exact: true })).toBeVisible();
  panel.unmount();

  setPageView({ scope: "all", trashed: false, board: false, list: null }, native.profile);
  const full = await render(PageHost);
  await add(full, "Second");
  await expect.element(full.getByText("Second", { exact: true })).toBeVisible();
  full.unmount();

  const again = await render(TaskHost, { profile: native.profile, scope: "all" });
  await expect.element(again.getByText("Second", { exact: true })).toBeVisible();
  await add(again, "Third");
  await expect.element(again.getByText("Third", { exact: true })).toBeVisible();

  await again.getByText("First", { exact: true }).hover();
  await again.getByRole("button", { name: "More actions" }).first().click();
  await again.getByRole("menuitem", { name: "Pin", exact: true }).click();
  await expect.element(again.getByLabelText("Pinned").first()).toBeVisible();
});

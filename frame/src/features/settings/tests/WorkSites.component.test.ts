import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkSiteAccessV1, WorkSiteRowV1 } from "$shared/ipc/bindings";
import SitesPage from "../components/sections/SitesPage.svelte";

const native = vi.hoisted(() => {
  const rows: WorkSiteRowV1[] = [
    { site: "slack.com", name: "Slack", access: "always", sensitive: false },
    { site: "chase.com", name: "chase.com", access: "ask", sensitive: true },
  ];
  return {
    rows,
    set: vi.fn(
      async (profile: string, change: { site: string; access: WorkSiteAccessV1 | null }) => {
        const sites = rows
          .filter((row) => row.site !== change.site)
          .concat(
            change.access
              ? [{ site: change.site, name: change.site, access: change.access, sensitive: false }]
              : [],
          );
        return { version: 1, profile, sites, error: null };
      },
    ),
  };
});
vi.mock("$domain/tabs", () => ({
  tabs: { profile: () => ({ id: "00000000000000000000000001", kind: "regular" }) },
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    faviconProbe: async () => true,
    workSites: vi.fn(async (profile: string) => ({
      version: 1,
      profile,
      sites: native.rows,
      error: null,
    })),
    workSetSite: native.set,
  });
});

test("a site's standing answer changes, a sensitive site can't be Always, and a typed site is added", async () => {
  await page.viewport(1000, 800);
  const screen = await render(SitesPage);
  await expect.element(screen.getByText("Slack", { exact: true })).toBeVisible();
  const chase = screen.getByRole("radiogroup", { name: "How the agent uses chase.com" });
  await expect.element(chase.getByRole("radio", { name: "Always" })).toBeDisabled();
  await screen
    .getByRole("radiogroup", { name: "How the agent uses Slack" })
    .getByRole("radio", { name: "Never" })
    .click();
  expect(native.set).toHaveBeenLastCalledWith("00000000000000000000000001", {
    site: "slack.com",
    access: "never",
  });
  await screen.getByRole("textbox", { name: "Add a site" }).fill("https://app.notion.so/workspace");
  await screen.getByRole("button", { name: "Add", exact: true }).click();
  expect(native.set).toHaveBeenLastCalledWith("00000000000000000000000001", {
    site: "notion.so",
    access: "always",
  });
  await screen.getByRole("button", { name: "Forget chase.com" }).click();
  expect(native.set).toHaveBeenLastCalledWith("00000000000000000000000001", {
    site: "chase.com",
    access: null,
  });
});

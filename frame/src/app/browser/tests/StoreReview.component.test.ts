import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { emitNativeEvent } from "$shared/testing/native-events";
import { revision, tabFixture } from "$shared/testing/fixtures";
import { tabs } from "$domain/tabs";
import { extensions } from "$domain/extensions";
import { surface } from "$domain/surface";
import StoreReviewHarness from "./StoreReviewHarness.svelte";

const storeId = "aeblfdkhhhdcdjpifhhbdiojplfjncoa";
const { prepare, setVisible, runCommand } = vi.hoisted(() => ({
  prepare: vi.fn(async (_tabId: string) => ({
    status: "ok" as const,
    data: "aeblfdkhhhdcdjpifhhbdiojplfjncoa",
  })),
  setVisible: vi.fn(async (_visible: boolean) => true),
  runCommand: vi.fn(async (_command: string) => ({ accepted: true, operation_id: null })),
}));

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    tabsBootstrap: async () => {},
    extensionStorePrepare: prepare,
    extensionManagementSetVisible: setVisible,
    runCommand,
  });
});

afterEach(() => {
  extensions.dispose();
  tabs.dispose();
  surface.dispose();
  prepare.mockClear();
  setVisible.mockClear();
  runCommand.mockClear();
});

test("a prepared store listing opens an accessible install review from a closed utility tray", async () => {
  await tabs.init();
  await extensions.init();
  await surface.init();
  const now = Date.now();
  emitNativeEvent("itemsChanged", {
    projection_revision: revision(now),
    profile: { id: "profile-a", name: "Personal", kind: "default" },
    spaces: [],
    active_space_id: null,
    nodes: [],
    tabs: [tabFixture({ url: `https://chromewebstore.google.com/detail/${storeId}` })],
    active: "tab-1",
    split_group: null,
  });
  emitNativeEvent("extensionManagementAvailabilityChanged", {
    projection_revision: revision(now + 1),
    availability: "configured",
  });

  const screen = await render(StoreReviewHarness);
  const tray = screen.getByRole("button", { name: "Utilities" });
  await expect.element(tray).toHaveAttribute("aria-expanded", "false");
  await screen.getByRole("button", { name: "Add to Zephium" }).click();
  await expect.poll(() => prepare.mock.calls.length).toBe(1);
  expect(prepare).toHaveBeenCalledWith("tab-1");
  await expect
    .poll(() => runCommand.mock.calls.some(([command]) => command === "browser.extensions"))
    .toBe(true);
  emitNativeEvent("itemsChanged", {
    projection_revision: revision(now + 2),
    profile: { id: "profile-a", name: "Personal", kind: "default" },
    spaces: [],
    active_space_id: null,
    nodes: [],
    tabs: [
      tabFixture({ url: `https://chromewebstore.google.com/detail/${storeId}` }),
      tabFixture({ id: "extensions-tab", title: "Extensions", url: null, content: "extensions" }),
    ],
    active: "extensions-tab",
    split_group: null,
  });
  emitNativeEvent("uiCommand", "browser.extensions");
  await expect.poll(() => setVisible.mock.calls.some(([visible]) => visible)).toBe(true);

  emitNativeEvent("extensionManagementChanged", {
    projection_revision: revision(now + 3),
    profile_id: "profile-a",
    phase: "ready",
    catalog_revision: "0000000000000001",
    profile_policy: {
      revision: "0000000000000001",
      paused: false,
      denied_site_count: 0,
      current_site_available: true,
      current_site_denied: false,
    },
    entries: [],
    candidates: [
      {
        candidate_index: 0,
        name: "1Password",
        description: null,
        author: null,
        version: "1.0",
        source: "external_compatibility",
        verified_catalog_unix: null,
        provenance: {
          source_url: `https://chromewebstore.google.com/detail/${storeId}`,
          upstream_version: "1.0",
          license_expression: "NOASSERTION",
          attribution: "1Password",
        },
        required_api: ["storage"],
        required_hosts: [],
        optional_api: [],
        optional_hosts: [],
        supports_file_access: false,
        file_access_available: false,
        private_access_available: false,
        compatibility: "degraded",
        limitations: [],
      },
    ],
    pending_update: null,
  });

  await expect.poll(() => extensions.management("profile-a")?.phase).toBe("ready");
  await expect
    .poll(() => document.querySelector<HTMLElement>("#extension-manager")?.getAttribute("role"))
    .toBe("region");
  const page = document.querySelector<HTMLElement>("#extension-manager")!;
  expect(page.closest("[inert]")).toBeNull();
  expect(page.closest(".ui-disclosure-panel")).toBeNull();
  await expect.poll(() => page.textContent?.includes("Required access")).toBe(true);
  expect(
    [...page.querySelectorAll("button")].some((button) => button.textContent?.trim() === "Install"),
  ).toBe(true);
  expect(page.querySelector('[aria-modal="true"]')).toBeNull();
});

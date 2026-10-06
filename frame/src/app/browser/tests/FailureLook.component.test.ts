import { afterEach, expect, test, vi } from "vitest";
import { page } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import "$styles/global.css";
import { emitNativeEvent } from "$shared/testing/native-events";
import { revision, tabFixture } from "$shared/testing/fixtures";
import { favicons } from "$domain/favicons";
import { surface } from "$domain/surface";
import { tabs } from "$domain/tabs";
import * as sidebar from "$session/sidebar-mode.svelte";
import * as launch from "$session/motion.svelte";
import Shell from "../Shell.svelte";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    tabsBootstrap: async () => {},
    sidebarSetWidth: async () => {},
    sidebarResize: async () => false,
    sidebarResizeGuide: async () => true,
    settingSet: async () => ({ accepted: true, operation_id: null }),
  });
});

afterEach(() => {
  sidebar.adoptMode("default");
  favicons.dispose();
  surface.dispose();
  tabs.dispose();
});

const shot = (name: string) => page.screenshot({ path: `../../../../../target/look/${name}.png` });
const settle = () => new Promise((resolve) => setTimeout(resolve, 450));

test("a first load that failed explains itself beside the sidebar", async () => {
  await page.viewport(1100, 720);
  document.documentElement.dataset.theme = "dark";
  document.body.style.background = "#232326";
  launch.dispose();
  await surface.init();
  await tabs.init();
  await favicons.init();
  const open = tabFixture({ id: "open", title: "Example", url: "https://example.com/" });
  const failed = tabFixture({
    id: "failed",
    title: "New Tab",
    url: null,
    failure: { url: "http://localhost:3000/", reason: "unreachable" },
  });
  emitNativeEvent("itemsChanged", {
    projection_revision: revision(20),
    profile: null,
    spaces: [],
    active_space_id: null,
    nodes: [open, failed].map((entry) => ({
      id: entry.id,
      parent_id: null,
      section: "today" as const,
      kind: { type: "tab" as const, tab_id: entry.id },
    })),
    tabs: [open, failed],
    active: "failed",
    split_group: null,
  });
  const screen = await render(Shell);
  await settle();
  await shot("failure-first-load");
  // The card stands in the content pane; the sidebar stays usable beside it.
  const card = document.querySelector("[role=alert] h1")!.closest("[role=alert]")!;
  const sidebarEdge = document.querySelector("aside.browser-sidebar")!.getBoundingClientRect().right;
  expect(card.getBoundingClientRect().left).toBeGreaterThanOrEqual(sidebarEdge);
  screen.unmount();
  emitNativeEvent("tabChanged", { ...failed, projection_revision: revision(21), failure: null });
  await render(Shell);
  await settle();
  await shot("failure-new-tab");
});

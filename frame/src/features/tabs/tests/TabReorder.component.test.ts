import { afterEach, expect, test, vi } from "vitest";
import { page } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import { flushSync } from "svelte";
import "$styles/global.css";
import type { SidebarNodeView } from "$shared/ipc/bindings";
import { emitNativeEvent } from "$shared/testing/native-events";
import { revision, tabFixture } from "$shared/testing/fixtures";
import { tabs } from "$domain/tabs";
import TabList from "../components/TabList.svelte";
import { sidebarTree } from "../lib/sidebar-model";

const native = vi.hoisted(() => ({
  setEssential: vi.fn(async () => ({ accepted: true, operation_id: null })),
  split: vi.fn(async () => ({ accepted: true, operation_id: null })),
}));

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    tabsBootstrap: async () => {},
    tabsSetEssential: native.setEssential,
    tabsSplit: native.split,
    tabDragOver: async () => undefined,
    tabDrop: async () => ({ accepted: true, operation_id: null }),
  });
});

vi.mock("$domain/operations", () => ({
  settle: async () => ({ outcome: "succeeded" }),
}));

afterEach(() => {
  tabs.dispose();
  native.setEssential.mockClear();
});

const open = ["a", "b", "c", "d"].map((id) => tabFixture({ id, title: `Tab ${id}` }));

async function list() {
  await page.viewport(400, 400);
  await tabs.init();
  const nodes: SidebarNodeView[] = open.map((tab) => ({
    id: tab.id,
    parent_id: null,
    section: "today",
    kind: { type: "tab", tab_id: tab.id },
  }));
  emitNativeEvent("itemsChanged", {
    projection_revision: revision(Date.now()),
    profile: null,
    spaces: [],
    active_space_id: null,
    nodes,
    tabs: open,
    active: "a",
    split_group: null,
  });
  flushSync();
  const tree = sidebarTree(nodes, open);
  const screen = await render(TabList, {
    entries: tree.today,
    section: "today",
    label: "Open tabs",
    splitting: false,
    onSelect: () => {},
  });
  const row = (id: string) =>
    screen.container.querySelector<HTMLElement>(`.browse-tab[data-zephium-tab-id="${id}"]`)!;
  return { row };
}

function drag(handle: HTMLElement, to: { x: number; y: number }) {
  const from = handle.getBoundingClientRect();
  const start = { x: from.left + 20, y: from.top + from.height / 2 };
  const init = { bubbles: true, pointerId: 1, button: 0, isPrimary: true };
  handle.dispatchEvent(
    new PointerEvent("pointerdown", { ...init, clientX: start.x, clientY: start.y }),
  );
  handle.dispatchEvent(
    new PointerEvent("pointermove", { ...init, clientX: start.x, clientY: start.y + 8 }),
  );
  handle.dispatchEvent(new PointerEvent("pointermove", { ...init, clientX: to.x, clientY: to.y }));
  return { init, handle };
}

const frame = () => new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

test("dragging a row down its list opens a slot and drops it there", async () => {
  const { row } = await list();
  const handle = row("a").querySelector<HTMLElement>(".tab-open")!;
  const target = row("c").getBoundingClientRect();
  const { init } = drag(handle, { x: target.left + 20, y: target.bottom - 4 });
  await frame();

  // The row itself follows; the rows it passed have moved up to make room.
  expect(row("a").hasAttribute("data-lifted")).toBe(true);
  expect(row("b").style.transform).toContain("translateY(-");
  expect(row("c").style.transform).toContain("translateY(-");
  expect(row("d").style.transform).toBe("");
  expect(document.querySelector(".pointer-events-none.fixed")).toBeNull();

  handle.dispatchEvent(
    new PointerEvent("pointerup", {
      ...init,
      clientX: target.left + 20,
      clientY: target.bottom - 4,
    }),
  );
  await expect.poll(() => native.setEssential.mock.calls.length).toBe(1);
  expect(native.setEssential).toHaveBeenCalledWith("a", false, "d");
  expect(native.split).not.toHaveBeenCalled();
});

test("letting go where it started moves nothing and puts every row back", async () => {
  const { row } = await list();
  const handle = row("b").querySelector<HTMLElement>(".tab-open")!;
  const home = row("b").getBoundingClientRect();
  const { init } = drag(handle, { x: home.left + 20, y: home.top + home.height / 2 + 4 });
  await frame();
  handle.dispatchEvent(
    new PointerEvent("pointerup", { ...init, clientX: home.left + 20, clientY: home.top + 20 }),
  );
  expect(native.setEssential).not.toHaveBeenCalled();
  await expect.poll(() => row("b").hasAttribute("data-lifted")).toBe(false);
  await expect.poll(() => row("b").style.transform).toBe("");
});

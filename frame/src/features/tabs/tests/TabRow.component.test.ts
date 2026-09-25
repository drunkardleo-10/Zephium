import { expect, test, vi } from "vitest";
import { flushSync } from "svelte";
import { render } from "vitest-browser-svelte";
import { tabFixture, revision } from "$shared/testing/fixtures";
import { emitNativeEvent } from "$shared/testing/native-events";
import { events } from "$shared/ipc/native-events";
import TabRow from "../components/TabRow.svelte";

test("presentation sentinels expose the exact title and revision synchronously", async () => {
  const tab = tabFixture({ title: "  <img onerror=unsafe()> & title  " });
  const screen = await render(TabRow, {
    tab,
    active: true,
    splitCandidate: false,
    onSelect: vi.fn(),
    onClose: vi.fn(),
    onContextMenu: vi.fn(),
    onPointerDown: vi.fn(),
    onPointerMove: vi.fn(),
    onPointerUp: vi.fn(),
    onPointerCancel: vi.fn(),
  });
  const row = screen.container.querySelector("[data-zephium-tab-id]")!;
  expect(row.getAttribute("data-zephium-tab-id")).toBe(tab.id);
  expect(row.getAttribute("data-zephium-tab-url")).toBe(tab.url);
  expect(row.getAttribute("data-zephium-projection-revision")).toBe(tab.projection_revision);
  const label = row.querySelector("[data-zephium-tab-label]")!;
  expect(label.textContent).toBe(tab.title);
  expect(label.childElementCount).toBe(0);
  // Validate the production scoped transport, including synchronous delivery.
  const received = vi.fn();
  const stop = await events.presentationTab.listen(received);
  const next = {
    tab: { ...tab, title: "Changed", projection_revision: revision(2) },
    active: tab.id,
  };
  flushSync(() => emitNativeEvent("presentationTab", next));
  expect(received).toHaveBeenCalledExactlyOnceWith({ payload: next });
  stop();
  emitNativeEvent("presentationTab", next);
  expect(received).toHaveBeenCalledTimes(1);
});

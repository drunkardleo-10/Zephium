import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import WorkCanvas from "../components/WorkCanvas.svelte";
import type { CanvasItem } from "../lib/canvas-model";

const request = (id: string, title: string): CanvasItem => ({
  id,
  type: "request",
  kind: "",
  title,
  detail: "",
  status: "",
  size: { width: 320, height: 80 },
});

test("a request just sent stands in the middle of what can be seen, clear of the island and the composer", async () => {
  await page.viewport(1200, 800);
  document.documentElement.dataset.reduceMotion = "true";
  const first = request("first", "Plan my trip");
  const screen = await render(WorkCanvas, {
    items: [first],
    links: [],
    authoritative: new Set(),
    initialView: {
      positions: { first: { x: 0, y: 0 }, second: { x: 0, y: 1400 } },
      viewport: { x: 40, y: 40, zoom: 1 },
    },
    fitTopInset: 56,
    fitBottomInset: 160,
    oninspect: vi.fn(),
  });
  screen.container.style.width = "1200px";
  screen.container.style.height = "800px";
  await new Promise((done) => setTimeout(done, 300));
  await screen.rerender({ items: [first, request("second", "Now the flights")] });
  const card = () =>
    screen.container.querySelector('.svelte-flow__node[data-id="second"]')?.getBoundingClientRect();
  const host = screen.container.getBoundingClientRect();
  await expect
    .poll(() => {
      const rect = card();
      return rect ? Math.round(rect.top + rect.height / 2 - host.top) : null;
    })
    .toBe(Math.round(56 + (800 - 160 - 56) / 2));
  const rect = card()!;
  expect(Math.round(rect.left + rect.width / 2 - host.left)).toBe(600);
  document.documentElement.dataset.reduceMotion = "false";
  await screen.unmount();
});

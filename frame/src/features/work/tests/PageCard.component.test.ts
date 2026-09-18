import "$styles/global.css";
import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import PageCard from "../components/cards/PageCard.svelte";
import type { CanvasItem } from "../lib/canvas-model";

const GIF = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
const PNG =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

function card(frame: string | null): CanvasItem {
  return {
    id: "page",
    type: "page",
    kind: "Page",
    title: "shop.example",
    detail: "https://shop.example/p/1",
    status: "Read",
    page: { url: "https://shop.example/p/1", host: "shop.example", frame, live: false },
  };
}

test("a settled page shows its frame on first mount and follows it when it changes", async () => {
  await page.viewport(1200, 800);
  const screen = await render(PageCard, { item: card(GIF), selected: false });
  const frame = screen.container.querySelector<HTMLImageElement>(".frame img");
  expect(frame?.getAttribute("src")).toBe(GIF);
  expect(frame?.getAttribute("loading")).toBe("eager");
  await screen.rerender({ item: card(PNG), selected: false });
  expect(screen.container.querySelector<HTMLImageElement>(".frame img")?.getAttribute("src")).toBe(
    PNG,
  );
  // Nothing recorded yet: the card keeps the host's initial, not a broken image.
  await screen.rerender({ item: card(null), selected: false });
  expect(screen.container.querySelector(".frame img")).toBeNull();
  expect(screen.container.querySelector(".placeholder")?.textContent).toBe("s");
  await screen.unmount();
});

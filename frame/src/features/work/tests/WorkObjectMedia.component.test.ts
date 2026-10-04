import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import ObjectsSheet from "./ObjectsSheet.svelte";
// Compile the lazy renderer before the interaction readiness deadline starts.
import "../components/objects/Media.svelte";
import type { MediaView } from "../lib/board/types";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

const video = (id: string, watch: string): MediaView => ({
  kind: "media",
  id,
  media: "video",
  url: `https://www.youtube.com/watch?v=${watch}`,
  provider: "youtube",
  title: `Lecture ${id}`,
  duration: "17:42",
});

test("a video is its poster with YouTube's mark, and a click opens its page", async () => {
  await page.viewport(1200, 800);
  const link = vi.fn();
  const screen = await render(ObjectsSheet, {
    rows: [
      { object: video("one", "FgzyLoSkL5k"), width: 400 },
      {
        object: { ...video("two", "x"), url: "https://example.com/talk.mp4", provider: "file" },
        width: 400,
      },
    ],
    actions: { link },
    zooms: [1],
  });
  await expect
    .poll(() => screen.container.querySelectorAll("button.screen").length, { timeout: 8000 })
    .toBe(2);
  // YouTube's own mark on its video, the plain disc on any other.
  expect(screen.container.querySelectorAll("svg.youtube")).toHaveLength(1);
  expect(screen.container.querySelectorAll("svg.disc")).toHaveLength(1);
  await screen.getByRole("button", { name: "Play Lecture one" }).click();
  expect(link).toHaveBeenCalledWith("https://www.youtube.com/watch?v=FgzyLoSkL5k");
  // Nothing plays in the canvas itself.
  expect(screen.container.querySelector("iframe")).toBeNull();
});

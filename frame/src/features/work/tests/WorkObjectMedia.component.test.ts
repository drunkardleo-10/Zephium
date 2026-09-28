import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import ObjectsSheet from "./ObjectsSheet.svelte";
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

test("a video plays in place through YouTube's private embed, one at a time", async () => {
  await page.viewport(1200, 800);
  const screen = await render(ObjectsSheet, {
    rows: [
      { object: video("one", "FgzyLoSkL5k"), width: 400 },
      { object: video("two", "dQw4w9WgXcQ"), width: 400 },
    ],
    levels: ["full"],
  });
  const frames = () => [...screen.container.querySelectorAll("iframe")];
  await expect
    .poll(() => screen.container.querySelectorAll("button.play").length, { timeout: 8000 })
    .toBe(2);
  // Nothing is embedded until the person plays.
  expect(frames()).toHaveLength(0);
  await screen.getByRole("button", { name: "Play Lecture one" }).click();
  await expect.poll(() => frames().length).toBe(1);
  const first = frames()[0]!;
  expect(first.src).toMatch(
    /^https:\/\/www\.youtube-nocookie\.com\/embed\/FgzyLoSkL5k\?autoplay=1/u,
  );
  expect(first.getAttribute("referrerpolicy")).toBe("strict-origin-when-cross-origin");
  await screen.getByRole("button", { name: "Play Lecture two" }).click();
  await expect
    .poll(() => frames().map((frame) => frame.src.includes("dQw4w9WgXcQ")))
    .toEqual([true]);
});

import "$styles/global.css";
import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import Character from "../Character.svelte";
import Orb from "../Orb.svelte";
import Shimmer from "../Shimmer.svelte";

const running = () => document.getAnimations().filter((each) => each.playState === "running");

function animated(animation: Animation): string[] {
  const effect = animation.effect as KeyframeEffect | null;
  return [
    ...new Set(
      (effect?.getKeyframes() ?? []).flatMap((frame) =>
        Object.keys(frame).filter(
          (key) => !["offset", "computedOffset", "easing", "composite"].includes(key),
        ),
      ),
    ),
  ];
}

test("a live character and indicator move by transform and opacity only", async () => {
  await page.viewport(400, 300);
  const character = await render(Character, { kind: "browser", mood: "reading", size: 32 });
  const orb = await render(Orb, { size: 24 });
  await expect.poll(() => running().length).toBeGreaterThan(2);
  for (const animation of running())
    for (const property of animated(animation))
      expect(["transform", "opacity"]).toContain(property);
  await character.unmount();
  await orb.unmount();
  expect(running()).toHaveLength(0);
});

test("a character at rest, done or stopped runs no animation", async () => {
  for (const mood of ["rest", "done", "stopped"] as const) {
    const screen = await render(Character, { kind: "lead", mood, size: 24 });
    await new Promise((done) => setTimeout(done, 50));
    expect(running()).toHaveLength(0);
    await screen.unmount();
  }
});

test("off screen, a live character and indicator hold still", async () => {
  await page.viewport(400, 300);
  const screen = await render(Character, { kind: "research", mood: "searching", size: 32 });
  const orb = await render(Orb, { size: 20 });
  await expect.poll(() => running().length).toBeGreaterThan(0);
  for (const host of [screen.container, orb.container]) {
    host.style.position = "fixed";
    host.style.insetBlockStart = "4000px";
  }
  await expect.poll(() => running().length).toBe(0);
  for (const host of [screen.container, orb.container]) host.style.insetBlockStart = "0";
  await expect.poll(() => running().length).toBeGreaterThan(0);
  await screen.unmount();
  await orb.unmount();
});

test("working words shimmer by transform alone and stop off screen", async () => {
  await page.viewport(400, 300);
  const screen = await render(Shimmer, { text: "Searching flights" });
  await expect.poll(() => running().length).toBe(2);
  for (const animation of running())
    for (const property of animated(animation)) expect(property).toBe("transform");
  screen.container.style.position = "fixed";
  screen.container.style.insetBlockStart = "4000px";
  await expect.poll(() => running().length).toBe(0);
  await screen.unmount();
});

import { afterEach, beforeEach, expect, it, vi } from "vitest";

beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers();
  vi.stubGlobal("window", { matchMedia: () => ({ matches: false }) });
  vi.stubGlobal("document", { documentElement: { dataset: {} } });
});

afterEach(() => {
  vi.useRealTimers();
});

it("holds the cascade until the window is revealed, then runs it once", async () => {
  const launch = await import("../motion.svelte");
  expect(launch.launchState()).toBe("armed");
  launch.reveal();
  expect(launch.launchState()).toBe("running");
  vi.advanceTimersByTime(1000);
  expect(launch.launchState()).toBe("idle");
  launch.reveal();
  expect(launch.launchState()).toBe("idle");
});

it("never leaves rows held when the reveal does not come", async () => {
  const launch = await import("../motion.svelte");
  expect(launch.launchState()).toBe("armed");
  vi.advanceTimersByTime(2500);
  expect(launch.launchState()).toBe("idle");
});

it("does not arm at all when motion is reduced", async () => {
  vi.stubGlobal("window", { matchMedia: () => ({ matches: true }) });
  const launch = await import("../motion.svelte");
  expect(launch.launchState()).toBe("idle");
});

import { afterEach, expect, it, vi } from "vitest";
import { observe, pause } from "../observe";
afterEach(() => vi.useRealTimers());
it("aborts observation immediately and consumes a late rejection", async () => {
  vi.useFakeTimers();
  const controller = new AbortController();
  let reject!: (error: Error) => void;
  const request = new Promise<void>((_resolve, failure) => {
    reject = failure;
  });
  const pending = observe(request, 5000, controller.signal);
  controller.abort();
  await expect(pending).resolves.toEqual({ state: "aborted" });
  expect(vi.getTimerCount()).toBe(0);
  reject(new Error("late native response"));
  await Promise.resolve();
});
it("clears a retry delay when its owner is disposed", async () => {
  vi.useFakeTimers();
  const controller = new AbortController();
  const pending = pause(1000, controller.signal);
  controller.abort();
  await pending;
  expect(vi.getTimerCount()).toBe(0);
});

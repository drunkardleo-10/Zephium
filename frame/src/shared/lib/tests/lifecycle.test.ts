import { describe, expect, it, vi } from "vitest";
import { createLifecycle, listenAll } from "../lifecycle";

describe("document lifecycle", () => {
  it("rejects callbacks from disposed and superseded generations", () => {
    const lifecycle = createLifecycle();
    const first = lifecycle.begin();
    expect(lifecycle.isCurrent(first)).toBe(true);
    lifecycle.end();
    expect(lifecycle.isCurrent(first)).toBe(false);
    const second = lifecycle.begin();
    expect(lifecycle.isCurrent(first)).toBe(false);
    expect(lifecycle.isCurrent(second)).toBe(true);
  });
  it("releases registrations even when another registration rejects first", async () => {
    const stop = vi.fn();
    let register!: (stop: () => void) => void;
    const pending = listenAll([
      Promise.reject(new Error("registration failed")),
      new Promise<() => void>((resolve) => {
        register = resolve;
      }),
    ]);
    const rejected = expect(pending).rejects.toThrow("registration failed");
    register(stop);
    await rejected;
    expect(stop).toHaveBeenCalledOnce();
  });
});

it("releases remaining listeners if an earlier cleanup throws", async () => {
  const stop = vi.fn();
  await expect(
    listenAll([
      Promise.resolve(() => {
        throw new Error("cleanup failed");
      }),
      Promise.resolve(stop),
      Promise.reject(new Error("registration failed")),
    ]),
  ).rejects.toThrow("registration failed");
  expect(stop).toHaveBeenCalledOnce();
});

import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import type { Component } from "svelte";
import Harness from "./Harness.svelte";
import Loaded from "./Loaded.svelte";

test("load failures are bounded presentation text and retry the actual loader", async () => {
  const loader = vi
    .fn()
    .mockRejectedValueOnce(new Error("private payload must not appear"))
    .mockResolvedValue({ default: Loaded });
  const screen = await render(Harness, { loader });
  await expect.element(screen.getByRole("alert")).toHaveTextContent("View unavailable");
  expect(screen.container.textContent).not.toContain("private payload");
  await screen.getByRole("button", { name: "Retry" }).click();
  await expect.element(screen.getByText("Ready", { exact: true })).toBeVisible();
  expect(loader).toHaveBeenCalledTimes(2);
});

test("a stale loader result cannot replace the newer view", async () => {
  let finish!: (module: { default: Component<{ label: string }> }) => void;
  const old = new Promise<{ default: Component<{ label: string }> }>((resolve) => {
    finish = resolve;
  });
  const screen = await render(Harness, { loader: () => old });
  await expect.element(screen.getByRole("status")).toHaveTextContent("Loading view");
  await screen.rerender({ loader: async () => ({ default: Loaded }) });
  await expect.element(screen.getByText("Ready", { exact: true })).toBeVisible();
  finish({
    default: (() => {
      throw new Error("stale view mounted");
    }) as Component<{ label: string }>,
  });
  await Promise.resolve();
  await expect.element(screen.getByText("Ready", { exact: true })).toBeVisible();
});

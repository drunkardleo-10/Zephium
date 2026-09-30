import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import ContextManifest from "../components/composer/ContextManifest.svelte";
import type { WorkContextDisclosureV1 } from "$shared/ipc/bindings";

const native = vi.hoisted(() => ({ preview: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workContextPreview: native.preview });
});

const disclosure: WorkContextDisclosureV1 = {
  version: 1,
  environment: "env",
  environment_revision: "3",
  purpose: "public_read",
  total_bytes: 1500,
  items: [
    {
      element: "e-note",
      kind: "note",
      title: "Keyboard notes",
      revision: "12",
      digest: "a".repeat(64),
      bytes: 1000,
      truncated: false,
      visibility: "private",
    },
    {
      element: "e-art",
      kind: "artifact",
      title: "Keyboard comparison",
      revision: "art-9",
      digest: "b".repeat(64),
      bytes: 500,
      truncated: true,
      visibility: "public",
    },
  ],
};

test("the manifest previews exactly what Rust admitted and flags private context on a public read", async () => {
  native.preview.mockResolvedValue({ kind: "admitted", disclosure });
  const onreview = vi.fn();
  const selection = {
    environment: "env",
    items: [
      { element: "e-note", revision: "12" },
      { element: "e-art", revision: "art-9" },
    ],
  };
  const screen = await render(ContextManifest, {
    profile: "p",
    selection,
    purpose: "public_read",
    onreview,
  });
  await expect
    .element(screen.getByRole("button", { name: /Using 2 selected objects/ }))
    .toBeVisible();
  expect(native.preview).toHaveBeenCalledExactlyOnceWith("p", "public_read", selection);
  await expect.poll(() => onreview.mock.lastCall?.[0]).toBe(true);
  // A compact chip in the field; opened, it says what goes and why it needs review.
  await expect.element(screen.getByText("2 selected", { exact: true })).toBeVisible();
  await screen.getByRole("button", { name: /Using 2 selected objects/ }).click();
  await expect.element(screen.getByText(/Includes private objects/)).toBeVisible();
  await expect.element(screen.getByText("Keyboard notes")).toBeVisible();
  await expect.element(screen.getByText("Keyboard comparison")).toBeVisible();
  await expect.element(screen.getByText("shortened")).toBeVisible();
  await expect.element(screen.getByText("1.5 KB leaves the device.")).toBeVisible();
  await screen.unmount();
});

test("a stale selection is reported honestly instead of pretending admission", async () => {
  native.preview.mockResolvedValue({ kind: "refused", error: "conflict" });
  const onreview = vi.fn();
  const screen = await render(ContextManifest, {
    profile: "p",
    selection: { environment: "env", items: [{ element: "e-note", revision: "old" }] },
    purpose: "planning",
    onreview,
  });
  await expect.element(screen.getByText(/A selected object changed/)).toBeVisible();
  await expect.poll(() => onreview.mock.calls.length).toBe(1);
  expect(onreview).toHaveBeenLastCalledWith(false);
  await screen.unmount();
});

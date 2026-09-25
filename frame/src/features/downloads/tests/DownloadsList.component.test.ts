import { expect, test, vi } from "vitest";
import { page } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import type { DownloadCall, DownloadView } from "$shared/ipc/bindings";
import DownloadsList from "../components/DownloadsList.svelte";

const native = vi.hoisted(() => ({
  call: vi.fn(),
  listener: null as null | ((event: { payload: { profile: string } }) => void),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ downloadCall: native.call });
});
vi.mock("$shared/ipc/native-events", () => ({
  events: {
    downloadsChanged: {
      listen: (listener: typeof native.listener) => {
        native.listener = listener;
        return Promise.resolve(() => {
          native.listener = null;
        });
      },
    },
  },
}));

test("native completion replaces cancellation with ID-scoped open and reveal actions", async () => {
  const profile = "00000000000000000000000001";
  let entry: DownloadView = {
    id: "00000000000000000000000002",
    revision: "00000001",
    created_at: "1",
    filename: "fixture.txt",
    source: "https://example.com",
    source_is_context: false,
    state: "receiving",
    received: "10",
    total: "100",
    error: null,
  };
  native.call.mockImplementation(async (_profile: string, call: DownloadCall) => {
    if (call.kind === "list")
      return {
        kind: "page",
        cleanup: { running: false, error: null },
        entries: [entry],
        next: null,
        supported: true,
      };
    if (call.kind === "updates")
      return {
        kind: "updates",
        cleanup: { running: false, error: null },
        entries: [entry],
        removed: [],
      };
    return { kind: "applied" };
  });
  const screen = await render(DownloadsList, { profile });
  await expect.element(page.getByRole("button", { name: "Cancel", exact: true })).toBeVisible();
  await expect
    .element(page.getByRole("button", { name: "Open", exact: true }))
    .not.toBeInTheDocument();
  entry = { ...entry, revision: "00000002", state: "cancelling" };
  native.listener?.({ payload: { profile } });
  await expect.element(page.getByText("Cancelling…", { exact: true })).toBeVisible();
  await expect
    .element(page.getByRole("button", { name: "Cancel", exact: true }))
    .not.toBeInTheDocument();
  await expect
    .element(page.getByRole("button", { name: "Remove from list", exact: true }))
    .not.toBeInTheDocument();
  entry = { ...entry, revision: "00000003", state: "completed", received: "100" };
  native.listener?.({ payload: { profile } });
  await expect.element(page.getByRole("button", { name: "Open", exact: true })).toBeVisible();
  await expect
    .element(page.getByRole("button", { name: "Cancel", exact: true }))
    .not.toBeInTheDocument();
  await page.getByRole("button", { name: "Open", exact: true }).click();
  expect(native.call).toHaveBeenCalledWith(profile, { kind: "open", id: entry.id });
  await screen.unmount();
  expect(native.listener).toBeNull();
});

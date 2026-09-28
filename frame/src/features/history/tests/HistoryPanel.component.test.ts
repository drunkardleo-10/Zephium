import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import "$styles/global.css";
import type { HistoryCall, HistoryResponse, HistoryVisitView } from "$shared/ipc/bindings";
import HistoryPanel from "../components/HistoryPanel.svelte";

const native = vi.hoisted(() => ({
  history: vi.fn(),
  open: vi.fn(async () => ({ accepted: true, operation_id: null })),
}));

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    historyCall: native.history as never,
    browserOpenUrl: native.open as never,
  });
});

afterEach(() => {
  vi.clearAllMocks();
  delete document.documentElement.dataset.theme;
});

function visit(title: string, agoSeconds: number): HistoryVisitView {
  const at = Math.floor(Date.now() / 1000) - agoSeconds;
  return {
    id: String(at * 1000 + Math.floor(Math.random() * 1000)),
    url: `https://example.com/${encodeURIComponent(title)}`,
    title,
    visited_at: String(at),
    icon: null,
  };
}

function serve(all: HistoryVisitView[]) {
  native.history.mockImplementation(async (_profile: string, call: HistoryCall) => {
    if (call.kind !== "page") return { kind: "removed", count: 0 } satisfies HistoryResponse;
    const needle = call.query.toLowerCase();
    const visits = all.filter((entry) => !needle || entry.title.toLowerCase().includes(needle));
    return { kind: "page", visits, next: null } satisfies HistoryResponse;
  });
}

test("lists recent pages in the sidebar", async () => {
  serve([visit("Rust Book", 60), visit("Release Notes", 120)]);

  render(HistoryPanel, { profile: "profile", query: "" });

  await expect.element(page.getByText("Rust Book")).toBeVisible();
  await expect.element(page.getByText("Today")).toBeVisible();
});

test("takes its query from the frame that owns the field", async () => {
  serve([visit("Rust Book", 60), visit("Release Notes", 120)]);

  // ToolFrame owns the search input and persists its value per tool; the panel
  // only mirrors it into the session.
  render(HistoryPanel, { profile: "profile", query: "rust" });

  await expect.element(page.getByText("Rust Book")).toBeVisible();
  // The first request already carries the query, so the unfiltered list never
  // appears and then vanishes.
  expect(native.history.mock.calls[0]?.[1]).toMatchObject({ query: "rust" });
  expect(page.getByText("Release Notes").elements()).toHaveLength(0);
});

test("opens a page in the window the panel belongs to, not a tab it names", async () => {
  const only = visit("Rust Book", 60);
  serve([only]);
  render(HistoryPanel, { profile: "profile", query: "" });
  await expect.element(page.getByText("Rust Book")).toBeVisible();

  await page.getByText("Rust Book").click();

  await vi.waitFor(() => expect(native.open).toHaveBeenCalledWith(only.url, false));
});

test("says so when a profile has nothing recorded", async () => {
  serve([]);

  render(HistoryPanel, { profile: "empty", query: "" });

  await expect.element(page.getByText("Your browsing history belongs here")).toBeVisible();
});

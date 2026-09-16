import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import "$styles/global.css";
import "@fontsource-variable/inter";
import type { HistoryCall, HistoryResponse, HistoryVisitView } from "$shared/ipc/bindings";
import HistoryPage from "../components/HistoryPage.svelte";

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

vi.mock("$domain/tabs", () => ({
  tabs: { profile: () => ({ id: "profile", name: "Personal", kind: "default" }) },
}));

vi.mock("$domain/surface", () => ({
  surface: { open: vi.fn(async () => {}) },
}));

afterEach(() => {
  vi.clearAllMocks();
  delete document.documentElement.dataset.theme;
});

const DAY = 24 * 60 * 60;

function visit(title: string, agoSeconds: number, host = "example.com"): HistoryVisitView {
  const at = Math.floor(Date.now() / 1000) - agoSeconds;
  return {
    id: String(at * 1000 + Math.floor(Math.random() * 1000)),
    url: `https://${host}/${encodeURIComponent(title)}`,
    title,
    visited_at: String(at),
    icon: null,
  };
}

/** Serves pages from a fixed list, honouring the cursor the surface sends. */
function serve(all: HistoryVisitView[]) {
  let removed: string[] = [];
  native.history.mockImplementation(async (_profile: string, call: HistoryCall) => {
    if (call.kind === "forget") {
      removed = [...removed, ...call.urls];
      return { kind: "removed", count: call.urls.length } satisfies HistoryResponse;
    }
    if (call.kind === "clear") {
      removed = all.map((entry) => entry.url);
      return { kind: "removed", count: all.length } satisfies HistoryResponse;
    }
    const needle = call.query.toLowerCase();
    const matching = all
      .filter((entry) => !removed.includes(entry.url))
      .filter((entry) => !needle || entry.title.toLowerCase().includes(needle))
      .filter((entry) => !call.before || Number(entry.id) < Number(call.before));
    const visits = matching.slice(0, call.limit);
    return {
      kind: "page",
      visits,
      next: visits.length === call.limit ? (visits.at(-1)?.id ?? null) : null,
    } satisfies HistoryResponse;
  });
}

test("groups visits under the day they happened", async () => {
  serve([visit("Rust Book", 60), visit("Release Notes", 2 * 60), visit("Old Article", DAY + 3600)]);
  render(HistoryPage);

  await expect.element(page.getByText("Rust Book")).toBeVisible();
  await expect.element(page.getByText("Today")).toBeVisible();
  await expect.element(page.getByText("Yesterday")).toBeVisible();
});

test("narrows the list to what the reader typed", async () => {
  serve([visit("Rust Book", 60), visit("Release Notes", 120)]);
  render(HistoryPage);
  await expect.element(page.getByText("Release Notes")).toBeVisible();

  await userEvent.fill(page.getByRole("searchbox"), "rust");

  await expect.element(page.getByText("Rust Book")).toBeVisible();
  await vi.waitFor(() => expect(page.getByText("Release Notes").elements()).toHaveLength(0));
});

test("a forgotten address leaves and does not come back with the next page", async () => {
  serve([visit("Forgettable", 60), visit("Keepsake", 120)]);
  render(HistoryPage);
  await expect.element(page.getByText("Forgettable")).toBeVisible();

  await page.getByText("Forgettable").hover();
  await page.getByRole("button", { name: "Remove from history" }).first().click();

  await vi.waitFor(() => expect(page.getByText("Forgettable").elements()).toHaveLength(0));
  await expect.element(page.getByText("Keepsake")).toBeVisible();
});

test("clearing a range empties the list", async () => {
  serve([visit("Rust Book", 60), visit("Release Notes", 120)]);
  render(HistoryPage);
  await expect.element(page.getByText("Rust Book")).toBeVisible();

  await page.getByRole("button", { name: "Clear" }).click();

  await expect.element(page.getByText("Your browsing history belongs here")).toBeVisible();
});

test("opening a row navigates in place rather than naming a tab", async () => {
  const only = visit("Rust Book", 60);
  serve([only]);
  render(HistoryPage);
  await expect.element(page.getByText("Rust Book")).toBeVisible();

  await page.getByText("Rust Book").click();

  await vi.waitFor(() => expect(native.open).toHaveBeenCalledWith(only.url, false));
});

test("says so when nothing matches, and offers no rows to act on", async () => {
  serve([visit("Rust Book", 60)]);
  render(HistoryPage);
  await expect.element(page.getByText("Rust Book")).toBeVisible();

  await userEvent.fill(page.getByRole("searchbox"), "nothing here");

  await expect.element(page.getByText("No matching pages")).toBeVisible();
});

test("reports a failure instead of showing an empty list", async () => {
  native.history.mockResolvedValue({
    kind: "error",
    error: "unavailable",
  } satisfies HistoryResponse);
  render(HistoryPage);

  await expect.element(page.getByText("History could not be loaded.")).toBeVisible();
});

test("renders in both themes", async () => {
  // The library fills the browser stage, which is far wider than the default
  // test viewport.
  await page.viewport(1100, 720);
  serve([visit("Rust Book", 60), visit("Release Notes", 120), visit("Old Article", DAY + 60)]);
  render(HistoryPage);
  await expect.element(page.getByText("Rust Book")).toBeVisible();

  await page.screenshot({ path: "../../../../../target/history-page-dark.png" });
  document.documentElement.dataset.theme = "light";
  await page.screenshot({ path: "../../../../../target/history-page-light.png" });
});

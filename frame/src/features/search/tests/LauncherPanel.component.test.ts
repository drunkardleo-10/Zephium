import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import { Note01Icon, Clock01Icon } from "@hugeicons/core-free-icons";
import "$styles/panel.css";
import "@fontsource-variable/inter";
import { emitNativeEvent } from "$shared/testing/native-events";
import type { PanelState, SearchResult } from "$shared/ipc/bindings";
import LauncherPanel from "../components/LauncherPanel.svelte";

const native = vi.hoisted(() => ({
  context: {
    window_id: "window",
    profile_id: "profile",
    space_id: "space",
    session_id: "0000000000000001",
    request_id: "",
  },
  search: vi.fn(async (_query: string, _request: string) => true),
  run: vi.fn(async () => ({ accepted: true, operation_id: null })),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ launcherSearch: native.search, launcherRun: native.run });
});
afterEach(() => {
  vi.clearAllMocks();
  delete document.documentElement.dataset.theme;
});

const context: PanelState = {
  ...native.context,
  revision: "1",
  visible: true,
  route: { type: "search" },
  profile_name: "Personal",
  error: false,
  corner_radius: 20,
  position_restorable: true,
};
const destinations = [
  { kind: "notes" as const, label: "Notes", icon: Note01Icon },
  { kind: "history" as const, label: "History", icon: Clock01Icon },
];
const search = (title: string): SearchResult => ({
  kind: "search",
  title,
  detail: "DuckDuckGo",
  favicon: null,
  action: { type: "OpenUrl", url: `https://duckduckgo.com/?q=${title}` },
});

async function reply(query: string, results: SearchResult[]) {
  await vi.waitFor(() => expect(native.search.mock.calls.at(-1)?.[0]).toBe(query));
  const owner = { ...native.context, request_id: native.search.mock.calls.at(-1)![1] };
  emitNativeEvent("searchChanged", {
    context: owner,
    query,
    results,
    completion: null,
    pending: false,
  });
  return owner;
}

test("Enter before any answer arrives runs once, against that exact request", async () => {
  const screen = await render(LauncherPanel, { props: { context, destinations } });
  const input = screen.getByRole("combobox");
  await input.fill("rust");
  await userEvent.keyboard("{Enter}");
  const owner = await reply("rust", [search("rust")]);
  await vi.waitFor(() =>
    expect(native.run).toHaveBeenCalledExactlyOnceWith(search("rust").action, owner),
  );
  // A later answer for the same query must not replay the action.
  emitNativeEvent("searchChanged", {
    context: owner,
    query: "rust",
    completion: null,
    pending: false,
    results: [search("rust"), search("rust book")],
  });
  expect(native.run).toHaveBeenCalledTimes(1);
});

test("an obsolete answer cannot put its rows back on screen", async () => {
  const screen = await render(LauncherPanel, { props: { context, destinations } });
  await screen.getByRole("combobox").fill("rust");
  const owner = await reply("rust", [search("rust"), search("rust book")]);
  await screen.getByRole("combobox").fill("@notes rust");
  await reply("@notes rust", []);
  emitNativeEvent("searchChanged", {
    context: owner,
    query: "rust",
    completion: null,
    pending: false,
    results: [search("stale")],
  });
  await expect.element(screen.getByRole("option", { name: /stale/u })).not.toBeInTheDocument();
});

test("keeps its panel chrome and shows destinations only for an empty field", async () => {
  const screen = await render(LauncherPanel, { props: { context, destinations } });
  expect(screen.container.querySelector("header")).not.toBeNull();
  expect(screen.container.querySelector("footer")).not.toBeNull();
  await expect.element(screen.getByRole("button", { name: "Notes" })).toBeInTheDocument();

  await screen.getByRole("combobox").fill("rust");
  await reply("rust", [
    search("rust"),
    {
      ...search("Rust Book"),
      kind: "history",
      detail: "https://doc.rust-lang.org/book/",
      action: { type: "OpenUrl", url: "https://doc.rust-lang.org/book/" },
    },
    {
      ...search("Rust notes"),
      kind: "note",
      detail: "Note",
      action: { type: "OpenNote", id: "note" },
    },
  ]);
  // One heading per source, in a fixed order, and no repeated per-row badge.
  await expect
    .element(screen.getByRole("group", { name: "History", exact: true }))
    .toBeInTheDocument();
  await expect
    .element(screen.getByRole("group", { name: "Notes", exact: true }))
    .toBeInTheDocument();
  const row = screen.container.querySelector<HTMLElement>(".row")!;
  expect(row.getBoundingClientRect().height).toBe(36);

  await page.viewport(760, 560);
  screen.container.style.cssText = "width:720px;height:520px;background:var(--color-canvas)";
  document.documentElement.dataset.theme = "dark";
  await page
    .elementLocator(screen.container)
    .screenshot({ path: "../../../../../target/search-launcher-dark.png" });
  document.documentElement.dataset.theme = "light";
  await page
    .elementLocator(screen.container)
    .screenshot({ path: "../../../../../target/search-launcher-light.png" });
});

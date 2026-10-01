import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { page } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import "$styles/global.css";
import { emitNativeEvent } from "$shared/testing/native-events";
import { resourceTestServer } from "$shared/testing/resources/server";
import type { SearchResult } from "$shared/ipc/bindings";
import { preferences } from "$domain/preferences";
import { taskSession } from "$domain/resources";
import NewTabLook from "./NewTabLook.svelte";

const native = vi.hoisted(() => ({
  call: vi.fn(),
  search: vi.fn(async (_query: string, _context: unknown) => true),
  settings: {} as Record<string, string>,
  profile: "00000000000000000000000501",
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    resourceCall: native.call,
    settingGet: async (key: string) => native.settings[key] ?? null,
    newtabSearchContext: async () => ({
      window_id: "window",
      profile_id: native.profile,
      space_id: "space",
      session_id: "newtab:tab:1",
      request_id: "",
    }),
    newtabSearch: native.search,
    newtabRun: async () => ({ accepted: true, operation_id: null }),
    newtabCancel: async () => true,
  });
});
vi.mock("$domain/tabs", () => ({
  tabs: { profile: () => ({ id: native.profile, name: "Alex Rivera", kind: "default" }) },
}));
const ontasks = vi.fn();

// Mid-afternoon on a Monday.
const NOW = new Date(2026, 8, 28, 14, 32);
const day = (offset: number) => {
  const at = new Date(NOW);
  at.setDate(at.getDate() + offset);
  return `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`;
};

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(() => {
  vi.useRealTimers();
  preferences.dispose();
  delete document.documentElement.dataset.theme;
  delete document.documentElement.dataset.material;
});

async function setup(settings: Record<string, string> = {}, due = 3) {
  native.profile = String(BigInt(native.profile) + 1n).padStart(26, "0");
  native.settings = settings;
  native.call.mockImplementation(resourceTestServer(native.profile).call);
  await preferences.init();
  const tasks = taskSession(native.profile, "seed");
  for (let index = 0; index < due; index++)
    await tasks.create({ title: `Task ${index}`, dueDate: day(index === 0 ? -1 : 0) });
  await tasks.create({ title: "Later", dueDate: day(3) });
  await page.viewport(1320, 860);
  const screen = await render(NewTabLook, { ontasks });
  screen.container.style.cssText = "inline-size:1200px;block-size:780px;padding:6px";
  return screen;
}

type Screen = Awaited<ReturnType<typeof setup>>;
/** Every running animation has finished or been cancelled; a cancelled one
 *  rejects its `finished` promise, which is not a failure here. */
const settled = () =>
  Promise.all(
    document.getAnimations().map((animation) => animation.finished.catch(() => undefined)),
  );

async function capture(screen: Screen, name: string) {
  await settled();
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    screen.container.style.background = "var(--color-canvas)";
    await settled();
    await page
      .elementLocator(screen.container)
      .screenshot({ path: `../../../../../target/newtab/${name}-${theme}.png` });
  }
}

const result = (title: string, kind: SearchResult["kind"] = "tab"): SearchResult => ({
  kind,
  title,
  detail: "rust-lang.org",
  icon: null,
  action: { type: "OpenUrl", url: `https://rust-lang.org/${encodeURIComponent(title)}` },
});

test("the field hangs from the top, the name and the day are cut into the page", async () => {
  const screen = await setup({ "ui.newtab-clock-format": "12h" });
  await expect.element(screen.getByRole("img", { name: "Zephium" })).toBeVisible();
  await expect.element(screen.getByText("2:32 PM")).toBeVisible();
  await expect.element(screen.getByText("Monday, September 28")).toBeVisible();
  await expect.element(screen.getByRole("combobox")).toHaveFocus();

  const pane = screen.container.querySelector(".newtab")!.getBoundingClientRect();
  const field = screen.container.querySelector(".ui-search")!.getBoundingClientRect();
  expect(field.top - pane.top).toBeLessThan(12);
  const ground = screen.container.querySelector(".ground .page")!.getAttribute("d")!;
  expect(ground).toContain("A18 18 0 0 0");

  // The ground cuts the name's letters out exactly where the name stands.
  const mark = screen.getByRole("img", { name: "Zephium" }).element().getBoundingClientRect();
  const cut = screen.container.querySelector(".ground mask")!;
  expect(Number(cut.getAttribute("x")) + 2).toBeCloseTo(mark.left - pane.left, 0);
  expect(Number(cut.getAttribute("y")) + 2).toBeCloseTo(mark.top - pane.top, 0);
  expect(Number(cut.getAttribute("width")) - 4).toBeCloseTo(mark.width, 0);
  // And a round pane for each of the page's controls; the figures are cards
  // along the foot.
  expect(screen.container.querySelectorAll(".ground .pane")).toHaveLength(2);
  const card = screen.container.querySelector(".tile")!.getBoundingClientRect();
  expect(pane.bottom - card.bottom).toBe(28);

  // The figures: the blocker's and focus time stand in until they are
  // counted; what is due is read from Tasks, the overdue one included.
  await expect.element(screen.getByText("1,284")).toBeVisible();
  await expect.element(screen.getByText("2h 14m")).toBeVisible();
  await expect.element(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "134");
  const due = screen.getByRole("button", { name: /Due today 3 1 overdue/u });
  await expect.element(due).toBeVisible();
  await due.click();
  expect(ontasks).toHaveBeenCalledOnce();
  await expect.element(screen.getByRole("button", { name: "Customize New Tab" })).toBeVisible();
  await expect.element(screen.getByRole("button", { name: "Profile" })).toBeVisible();
  await capture(screen, "rest");

  // Over a material the openings are the window itself: a stand-in
  // wallpaper behind the pane shows through them and nowhere else.
  document.documentElement.dataset.material = "liquid_glass";
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    screen.container.style.background =
      "linear-gradient(115deg, #2b1c3a, #c2553f 38%, #e9a26b 52%, #2f6b5e 70%, #10222a)";
    await settled();
    await page
      .elementLocator(screen.container)
      .screenshot({ path: `../../../../../target/newtab/rest-glass-${theme}.png` });
  }
});

test("typing turns the page over to results dropping from the field", async () => {
  const screen = await setup();
  const input = screen.getByRole("combobox");
  await input.fill("rust");
  await vi.waitFor(() => expect(native.search).toHaveBeenCalled());
  const context = native.search.mock.calls.at(-1)![1];
  emitNativeEvent("searchChanged", {
    context: context as never,
    query: "rust",
    completion: null,
    pending: false,
    results: [result("rust", "search"), result("Rust docs"), result("The Rust Book", "history")],
  });
  await expect.element(screen.getByRole("option", { name: /Rust docs/u })).toBeVisible();
  await capture(screen, "search");
});

test("a narrow pane keeps the notch inside the page", async () => {
  const screen = await setup({}, 0);
  screen.container.style.inlineSize = "660px";
  await expect.element(screen.getByRole("combobox")).toBeVisible();
  const pane = screen.container.querySelector(".newtab")!.getBoundingClientRect();
  const dock = screen.container.querySelector(".field")!.getBoundingClientRect();
  expect(dock.left).toBeGreaterThan(pane.left);
  expect(dock.right).toBeLessThan(pane.right);
  // Nothing due, and nothing late.
  await expect
    .element(screen.getByRole("button", { name: /Due today 0 Nothing overdue/u }))
    .toBeVisible();
  await capture(screen, "narrow");
});

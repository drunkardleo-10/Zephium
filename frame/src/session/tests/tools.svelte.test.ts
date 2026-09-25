import { beforeEach, expect, it, vi } from "vitest";
const host = vi.hoisted(() => ({
  page: null as string | null,
  open: vi.fn(),
  extent: vi.fn(),
  stop: vi.fn(),
  listener: null as ((event: { payload: string }) => void) | null,
  listen: vi.fn(),
}));
vi.mock("$domain/surface", () => ({ surface: { currentPage: () => host.page, open: host.open } }));
vi.mock("../sidebar-mode.svelte", () => ({ setPanelExtent: host.extent }));
vi.mock("$shared/ipc/native-events", () => ({ events: { uiCommand: { listen: host.listen } } }));
beforeEach(() => {
  vi.resetModules();
  vi.resetAllMocks();
  host.page = null;
  host.listener = null;
  host.listen.mockImplementation((listener: typeof host.listener) => {
    host.listener = listener;
    return Promise.resolve(host.stop);
  });
});
it("opens at once and widens the column in the same step", async () => {
  const tools = await import("../tools.svelte");
  const first = tools.init();
  expect(tools.init()).toBe(first);
  await first;
  tools.open("notes");
  expect(tools.activeTool()).toBe("notes");
  expect(host.extent).toHaveBeenCalledWith(336);
  tools.close();
  expect(tools.activeTool()).toBeNull();
  expect(host.extent).toHaveBeenLastCalledWith(0);
  tools.dispose();
  expect(host.stop).toHaveBeenCalledOnce();
});
it("explicit close invalidates a queued tool before browser return settles", async () => {
  const tools = await import("../tools.svelte");
  await tools.init();
  host.page = "settings";
  tools.open("notes");
  tools.close();
  host.page = null;
  host.listener?.({ payload: "browser.return" });
  expect(tools.activeTool()).toBeNull();
  tools.dispose();
});
it("releases a subscription that resolves after disposal", async () => {
  let finish!: (stop: () => void) => void;
  host.listen.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const tools = await import("../tools.svelte");
  const pending = tools.init();
  tools.dispose();
  finish(host.stop);
  await pending;
  expect(host.stop).toHaveBeenCalledOnce();
});

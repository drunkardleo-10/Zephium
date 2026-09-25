import { beforeEach, expect, test, vi } from "vitest";
const host = vi.hoisted(() => ({
  ready: vi.fn(),
  request: null as null | ((event: { payload: string }) => void),
  cancel: null as null | ((event: { payload: string }) => void),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ resourceCloseReady: host.ready });
});
vi.mock("$shared/ipc/native-events", () => ({
  events: {
    resourceClose: {
      listen: (fn: typeof host.request) => {
        host.request = fn;
        return Promise.resolve(() => {});
      },
    },
    resourceCloseCancelled: {
      listen: (fn: typeof host.cancel) => {
        host.cancel = fn;
        return Promise.resolve(() => {});
      },
    },
  },
}));
beforeEach(() => {
  vi.resetModules();
  host.ready.mockReset().mockResolvedValue(true);
  vi.stubGlobal("document", { body: { inert: false } });
});
test("flushes before acknowledging close and ignores stale cancellation", async () => {
  const { registerCloseTask, installCloseService } = await import("../close");
  const flush = vi.fn().mockResolvedValue(true);
  const remove = registerCloseTask(flush);
  const stop = installCloseService();
  host.request?.({ payload: "current" });
  expect(document.body.inert).toBe(true);
  await vi.waitFor(() => expect(host.ready).toHaveBeenCalledWith("current", true));
  host.cancel?.({ payload: "old" });
  expect(document.body.inert).toBe(true);
  host.cancel?.({ payload: "current" });
  expect(document.body.inert).toBe(false);
  remove();
  stop();
});
test("a failed save refuses ordinary closing and restores editing", async () => {
  const { registerCloseTask, installCloseService } = await import("../close");
  const remove = registerCloseTask(async () => false);
  const stop = installCloseService();
  host.request?.({ payload: "current" });
  await vi.waitFor(() => expect(host.ready).toHaveBeenCalledWith("current", false));
  await vi.waitFor(() => expect(document.body.inert).toBe(false));
  remove();
  stop();
});

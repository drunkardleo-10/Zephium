import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSearchController, type SearchSnapshot } from "../lib/search-controller";
const owner = { window_id: "w", profile_id: "p", space_id: "s", session_id: "0000000000000001" };
describe("launcher request lifecycle", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());
  it("coalesces typing and makes no request during IME composition", async () => {
    const send = vi.fn(async () => true);
    const controller = createSearchController({
      owner,
      send,
      update: () => {},
      id: () => "request",
    });
    controller.compositionStart();
    controller.change("に");
    await vi.advanceTimersByTimeAsync(100);
    expect(send).not.toHaveBeenCalled();
    controller.compositionEnd("日本");
    controller.change("日本語");
    await vi.advanceTimersByTimeAsync(60);
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith("日本語", "request");
    controller.dispose();
    expect(vi.getTimerCount()).toBe(0);
  });
  it("cancels all owned timers when hidden and ignores a late response", async () => {
    const update = vi.fn();
    const send = vi.fn(async () => true);
    const controller = createSearchController({ owner, send, update, id: () => "request" });
    controller.start();
    await Promise.resolve();
    controller.dispose();
    update.mockClear();
    controller.receive({ context: { ...owner, request_id: "request" }, query: "", results: [] });
    await vi.runAllTimersAsync();
    expect(update).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });
  it("does not let an old request failure replace newer results", async () => {
    let reject!: (error: Error) => void;
    const snapshots: SearchSnapshot[] = [];
    let id = 0;
    const send = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise((_resolve, fail) => {
            reject = fail;
          }),
      )
      .mockResolvedValue(true);
    const controller = createSearchController({
      owner,
      send,
      update: (state) => snapshots.push(state),
      id: () => String(++id),
    });
    controller.start();
    controller.change("new");
    await vi.advanceTimersByTimeAsync(60);
    controller.receive({ context: { ...owner, request_id: "2" }, query: "new", results: [] });
    reject(new Error("old failure"));
    await Promise.resolve();
    expect(snapshots.at(-1)?.error).toBe("none");
    controller.dispose();
  });
  it("bounds the UTF-8 request before crossing IPC", () => {
    const send = vi.fn(async () => true);
    const update = vi.fn();
    const controller = createSearchController({ owner, send, update });
    controller.change("界".repeat(1000));
    vi.advanceTimersByTime(60);
    expect(send).not.toHaveBeenCalled();
    expect(update).toHaveBeenLastCalledWith({ results: [], pending: false, error: "too_long" });
    controller.dispose();
  });
});

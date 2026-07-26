import { beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => {
  let listener: ((event: { payload: string }) => void) | undefined;
  const stop = vi.fn();
  const listen = vi.fn((next: (event: { payload: string }) => void) => {
    listener = next;
    return Promise.resolve(stop);
  });

  return {
    listen,
    stop,
    emit(command: string) {
      listener?.({ payload: command });
    },
    reset() {
      listener = undefined;
      listen.mockClear();
      stop.mockClear();
    },
  };
});

vi.mock("../src/ipc/native-events", () => ({
  events: {
    uiCommand: {
      listen: native.listen,
    },
  },
}));

describe("trusted UI command state", () => {
  beforeEach(() => {
    vi.resetModules();
    native.reset();
  });

  it("publishes repeated split-selection requests with distinct sequences", async () => {
    const ui = await import("../src/state/ui.svelte");
    const first = ui.init();
    const second = ui.init();

    expect(first).toBe(second);
    await first;
    expect(native.listen).toHaveBeenCalledOnce();

    native.emit("split.choose");
    expect(ui.uiCommand()).toEqual({ id: "split.choose", seq: 1 });

    native.emit("split.choose");
    expect(ui.uiCommand()).toEqual({ id: "split.choose", seq: 2 });

    ui.dispose();
    ui.dispose();
    expect(native.stop).toHaveBeenCalledOnce();
  });
});

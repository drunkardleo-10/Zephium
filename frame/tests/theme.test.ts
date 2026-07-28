import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => {
  let commandListener: ((event: { payload: string }) => void) | undefined;

  return {
    order: [] as string[],
    attributes: new Map<string, string>(),
    media: {
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    },
    stop: vi.fn(),
    listen: vi.fn((listener: (event: { payload: string }) => void) => {
      harness.order.push("listen");
      commandListener = listener;
      return Promise.resolve(harness.stop);
    }),
    emit(command: string) {
      commandListener?.({ payload: command });
    },
    uiInfo: vi.fn(async () => {
      harness.order.push("uiInfo");
      return { material: true };
    }),
    settingGet: vi.fn(async () => {
      harness.order.push("settingGet");
      return "light";
    }),
  };
});

vi.mock("../src/shared/ipc/bindings", () => ({
  commands: {
    uiInfo: harness.uiInfo,
    settingGet: harness.settingGet,
  },
}));

vi.mock("../src/shared/ipc/native-events", () => ({
  events: {
    uiCommand: {
      listen: harness.listen,
    },
  },
}));

describe("theme startup lifecycle", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    harness.order.length = 0;
    harness.attributes.clear();
    harness.media.matches = false;
    harness.uiInfo.mockImplementation(async () => {
      harness.order.push("uiInfo");
      return { material: true };
    });
    harness.settingGet.mockImplementation(async () => {
      harness.order.push("settingGet");
      return "light";
    });

    vi.stubGlobal("navigator", { userAgent: "Mac" });
    vi.stubGlobal("window", {
      matchMedia: vi.fn(() => harness.media),
    });
    vi.stubGlobal("document", {
      documentElement: {
        setAttribute(name: string, value: string) {
          harness.attributes.set(name, value);
          harness.order.push(`${name}:${value}`);
        },
      },
    });
  });

  it("applies a deterministic theme and subscribes before native queries", async () => {
    const theme = await import("../src/domain/theme/theme");
    const first = theme.init();
    const second = theme.init();

    expect(first).toBe(second);
    expect(harness.order.slice(0, 3)).toEqual(["data-theme:dark", "listen", "uiInfo"]);

    await first;

    expect(harness.attributes.get("data-material")).toBe("vibrancy");
    expect(harness.attributes.get("data-theme")).toBe("light");
    expect(harness.media.addEventListener).toHaveBeenCalledOnce();
    expect(harness.listen).toHaveBeenCalledOnce();

    theme.dispose();
    theme.dispose();
    expect(harness.stop).toHaveBeenCalledOnce();
    expect(harness.media.removeEventListener).toHaveBeenCalledOnce();
  });

  it("does not let a stale stored preference overwrite a live theme command", async () => {
    let resolveStored: ((value: string) => void) | undefined;
    harness.settingGet.mockImplementation(
      () =>
        new Promise((resolve) => {
          harness.order.push("settingGet");
          resolveStored = resolve;
        }),
    );

    const theme = await import("../src/domain/theme/theme");
    const ready = theme.init();
    await vi.waitFor(() => expect(resolveStored).toBeTypeOf("function"));

    harness.emit("theme.dark");
    resolveStored?.("light");
    await ready;

    expect(harness.attributes.get("data-theme")).toBe("dark");
    theme.dispose();
  });

  it("removes a listener that resolves after disposal", async () => {
    let resolveInfo: ((value: { material: boolean }) => void) | undefined;
    harness.uiInfo.mockImplementation(
      () =>
        new Promise((resolve) => {
          harness.order.push("uiInfo");
          resolveInfo = resolve;
        }),
    );

    const theme = await import("../src/domain/theme/theme");
    const ready = theme.init();
    theme.dispose();
    resolveInfo?.({ material: true });
    await ready;

    expect(harness.stop).toHaveBeenCalledOnce();
    expect(harness.media.addEventListener).not.toHaveBeenCalled();
  });
});

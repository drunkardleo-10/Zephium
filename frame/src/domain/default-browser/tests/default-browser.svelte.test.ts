import { beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ status: vi.fn(), request: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    defaultBrowserStatus: native.status,
    defaultBrowserRequest: native.request,
  });
});

beforeEach(() => {
  vi.resetModules();
  vi.resetAllMocks();
});

describe("default browser", () => {
  it("reports what native read, keeping the last answer when a read fails", async () => {
    const defaultBrowser = await import("../default-browser.svelte");
    expect(defaultBrowser.current()).toBeNull();
    native.status.mockResolvedValueOnce({ is_default: false, can_request: true });
    await defaultBrowser.refresh();
    expect(defaultBrowser.current()).toEqual({ is_default: false, can_request: true });
    native.status.mockRejectedValueOnce(new Error("gone"));
    await defaultBrowser.refresh();
    expect(defaultBrowser.current()).toEqual({ is_default: false, can_request: true });
  });

  it("asks once at a time and shows the answer the system gave", async () => {
    const defaultBrowser = await import("../default-browser.svelte");
    let answer!: (value: unknown) => void;
    native.request.mockReturnValueOnce(new Promise((resolve) => (answer = resolve)));
    const first = defaultBrowser.request();
    expect(defaultBrowser.pending()).toBe(true);
    await defaultBrowser.request();
    expect(native.request).toHaveBeenCalledOnce();
    answer({ is_default: true, can_request: true });
    await first;
    expect(defaultBrowser.pending()).toBe(false);
    expect(defaultBrowser.current()?.is_default).toBe(true);
  });

  it("never lets an older read overwrite a newer one", async () => {
    const defaultBrowser = await import("../default-browser.svelte");
    let slow!: (value: unknown) => void;
    native.status
      .mockReturnValueOnce(new Promise((resolve) => (slow = resolve)))
      .mockResolvedValueOnce({ is_default: true, can_request: true });
    const older = defaultBrowser.refresh();
    await defaultBrowser.refresh();
    slow({ is_default: false, can_request: true });
    await older;
    expect(defaultBrowser.current()?.is_default).toBe(true);
  });
});

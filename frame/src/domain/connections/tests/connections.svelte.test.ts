import { afterEach, expect, test, vi } from "vitest";
import { ConnectionsSession } from "../connections.svelte";

const native = vi.hoisted(() => ({ list: vi.fn(), check: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workConnections: native.list, workCheckConnection: native.check });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("network restoration and a wake gap reconnect once and disposal removes listeners", async () => {
  vi.useFakeTimers();
  const window = new EventTarget();
  vi.stubGlobal("window", window);
  vi.stubGlobal("navigator", { onLine: true });
  native.list.mockResolvedValue({
    version: 1,
    profile: "p",
    clis: [],
    servers: [
      {
        server: {
          id: "notes",
          name: "Notes",
          enabled: true,
          transport: { kind: "http", url: "https://example.com/mcp", auth: "none" },
        },
        secrets: [],
        signed_in: false,
      },
    ],
    error: null,
  });
  native.check.mockResolvedValue({
    version: 1,
    profile: "p",
    id: "notes",
    outcome: "ready",
    server_name: null,
    tools: [],
    error: null,
  });
  const session = new ConnectionsSession("p");
  try {
    await session.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(native.check).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new Event("online"));
    window.dispatchEvent(new Event("online"));
    await vi.advanceTimersByTimeAsync(0);
    expect(native.check).toHaveBeenCalledTimes(2);
    vi.setSystemTime(Date.now() + 60000);
    await vi.advanceTimersByTimeAsync(15000);
    expect(native.check).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(30000);
    expect(native.check).toHaveBeenCalledTimes(3);
  } finally {
    session.dispose();
  }
  window.dispatchEvent(new Event("online"));
  await vi.advanceTimersByTimeAsync(60000);
  expect(native.check).toHaveBeenCalledTimes(3);
});

import { beforeEach, expect, it, vi } from "vitest";

const host = vi.hoisted(() => ({ dragOver: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ tabDragOver: host.dragOver });
});

beforeEach(() => {
  vi.resetModules();
  host.dragOver.mockReset().mockResolvedValue(undefined);
});

it("does not clear native drag state for an ordinary click", async () => {
  const drag = await import("../tab-drag.svelte");
  drag.end();
  expect(host.dragOver).not.toHaveBeenCalled();
});

it("clears an active drag exactly once", async () => {
  const drag = await import("../tab-drag.svelte");
  drag.begin("tab-1");
  drag.end();
  drag.end();
  expect(drag.draggedId()).toBeNull();
  expect(host.dragOver).toHaveBeenCalledExactlyOnceWith(null, null);
});

it("handles reset rejection without an unhandled promise", async () => {
  host.dragOver.mockRejectedValue(new Error("native transport unavailable"));
  const drag = await import("../tab-drag.svelte");
  drag.begin("tab-1");
  drag.end();
  await Promise.resolve();
  expect(drag.moveFailed()).toBe(true);
});

it("does not report a previous reset failure on a new drag", async () => {
  host.dragOver.mockRejectedValue(new Error("old reset"));
  const drag = await import("../tab-drag.svelte");
  drag.begin("tab-1");
  drag.end();
  drag.begin("tab-2");
  await Promise.resolve();
  expect(drag.moveFailed()).toBe(false);
  expect(drag.draggedId()).toBe("tab-2");
});

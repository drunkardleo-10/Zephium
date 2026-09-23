import { describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ width: vi.fn(async () => undefined) }));

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    sidebarSetWidth: native.width,
    settingSet: vi.fn(async () => ({ operation_id: null, accepted: true })),
    settingGet: vi.fn(async () => null),
  });
});

const {
  COMPACT_WIDTH,
  MAX_EXPANDED_WIDTH,
  MIN_EXPANDED_WIDTH,
  SNAP_THRESHOLD,
  applyDragWidth,
  resolveDragWidth,
  setPanelExtent,
  toggleMode,
} = await import("../sidebar-mode.svelte");

describe("resolveDragWidth", () => {
  it("snaps to the rail below the threshold", () => {
    for (const value of [0, 1, COMPACT_WIDTH, SNAP_THRESHOLD - 1]) {
      expect(resolveDragWidth(value).mode).toBe("compact");
    }
  });

  it("expands at and above the threshold", () => {
    expect(resolveDragWidth(SNAP_THRESHOLD).mode).toBe("default");
    expect(resolveDragWidth(300).mode).toBe("default");
  });

  it("never settles between the two designed shapes", () => {
    // Anything that resolves to `default` must be a legal expanded width, so a
    // drag can never leave the sidebar at an in-between size.
    for (let value = 0; value <= 600; value += 7) {
      const resolved = resolveDragWidth(value);
      if (resolved.mode === "default") {
        expect(resolved.expanded).toBeGreaterThanOrEqual(MIN_EXPANDED_WIDTH);
        expect(resolved.expanded).toBeLessThanOrEqual(MAX_EXPANDED_WIDTH);
      }
    }
  });

  it("treats a non-finite width as the rail rather than propagating it", () => {
    for (const value of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
      expect(resolveDragWidth(value).mode).toBe("compact");
    }
  });
});

describe("which width changes the page travels with", () => {
  const last = () => native.width.mock.calls.at(-1) as unknown as [number, boolean];

  it("slides for a deliberate change of shape and follows a drag directly", () => {
    toggleMode();
    expect(last()).toEqual([COMPACT_WIDTH, true]);
    toggleMode();
    expect(last()[1]).toBe(true);

    applyDragWidth(300);
    expect(last()).toEqual([300, false]);
    applyDragWidth(310);
    expect(last()).toEqual([310, false]);

    // Crossing the snap point is a change of shape, however it was reached.
    applyDragWidth(SNAP_THRESHOLD - 1);
    expect(last()).toEqual([COMPACT_WIDTH, true]);
    applyDragWidth(SNAP_THRESHOLD + 40);
    expect(last()[1]).toBe(true);
  });

  it("slides when a tool opens beside the rail and when it closes", () => {
    setPanelExtent(336);
    expect(last()[1]).toBe(true);
    setPanelExtent(0);
    expect(last()[1]).toBe(true);
  });
});

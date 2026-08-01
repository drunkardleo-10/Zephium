import { describe, expect, it, vi } from "vitest";

vi.mock("../src/shared/ipc/bindings", () => ({
  commands: {
    sidebarSetWidth: vi.fn(async () => undefined),
    settingSet: vi.fn(async () => ({ operation_id: null, accepted: true })),
    settingGet: vi.fn(async () => null),
  },
}));

const { COMPACT_WIDTH, MAX_EXPANDED_WIDTH, MIN_EXPANDED_WIDTH, SNAP_THRESHOLD, resolveDragWidth } =
  await import("../src/features/sidebar/sidebar-mode.svelte");

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

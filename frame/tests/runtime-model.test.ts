import { describe, expect, it } from "vitest";
import type { RuntimeStatus } from "../src/shared/ipc/bindings";
import { runtimeNotifications } from "../src/domain/runtime/runtime-model";

const clear: RuntimeStatus = {
  restart_required: false,
  security_advisories: [],
};

describe("runtime notifications", () => {
  it("stays absent when the admitted runtime needs no user action", () => {
    expect(runtimeNotifications(clear)).toEqual([]);
  });

  it("separates a known system update from an overdue Zephium review", () => {
    expect(
      runtimeNotifications({
        ...clear,
        security_advisories: [
          {
            kind: "update_recommended",
            update_target: "operating_system",
          },
        ],
      }),
    ).toEqual([
      expect.objectContaining({
        id: "update_operating_system",
        title: "System update recommended",
        tone: "warning",
      }),
    ]);

    expect(
      runtimeNotifications({
        ...clear,
        security_advisories: [
          {
            kind: "review_overdue",
            update_target: "zephium",
          },
        ],
      })[0]?.title,
    ).toBe("Security review is overdue");
  });

  it("retains both independent process-lifetime notices", () => {
    expect(
      runtimeNotifications({
        restart_required: true,
        security_advisories: [
          {
            kind: "review_overdue",
            update_target: "zephium",
          },
          {
            kind: "unreviewed_runtime",
            update_target: "zephium",
          },
        ],
      }).map((notification) => notification.id),
    ).toEqual(["restart_required", "review_overdue", "unreviewed_runtime"]);
  });

  it("deduplicates malformed repeats and treats unreviewed runtimes as warnings", () => {
    const notification = {
      kind: "unreviewed_runtime" as const,
      update_target: "zephium" as const,
    };
    expect(
      runtimeNotifications({
        restart_required: false,
        security_advisories: [notification, notification],
      }),
    ).toEqual([
      expect.objectContaining({
        id: "unreviewed_runtime",
        tone: "warning",
      }),
    ]);
  });
});

import { describe, expect, it } from "vitest";
import type { ExtensionActionFailedView, ExtensionActionsView } from "../src/shared/ipc/bindings";
import {
  ExtensionProjectionModel,
  failureForContext,
  initialExtensionActions,
} from "../src/domain/extensions/extensions-model";
import { ZERO_PROJECTION_REVISION } from "../src/domain/tabs/tabs-model";

function revision(value: number): string {
  return value.toString(16).padStart(32, "0");
}

function actions(value: number, label = "Bitwarden"): ExtensionActionsView {
  return {
    projection_revision: revision(value),
    profile_id: "profile-a",
    tab_id: "tab-a",
    actions: [
      {
        runtime: {
          install_id: "install-a",
          generation: "0000000000000001",
        },
        revision: "0000000000000002",
        label,
        badge: "1",
        icon_rgba_base64: null,
        enabled: true,
        presents_popup: true,
        unread_badge: true,
      },
    ],
  };
}

function failure(value: number): ExtensionActionFailedView {
  return {
    projection_revision: revision(value),
    profile_id: "profile-a",
    tab_id: "tab-a",
    reason: "tab_discarded",
  };
}

describe("extension projection admission", () => {
  it("starts with an exact empty replacement at the zero revision", () => {
    expect(initialExtensionActions()).toEqual({
      projection_revision: ZERO_PROJECTION_REVISION,
      profile_id: "",
      tab_id: null,
      actions: [],
    });
  });

  it("replaces actions only with a strictly newer actor revision", () => {
    const model = new ExtensionProjectionModel();
    const current = actions(3);

    expect(model.applyActions(current)).toBe(true);
    expect(model.actions).toBe(current);
    expect(model.applyActions(actions(3, "Duplicate"))).toBe(false);
    expect(model.applyActions(actions(2, "Stale"))).toBe(false);
    expect(model.actions).toBe(current);
  });

  it("uses one revision floor for action and failure event streams", () => {
    const model = new ExtensionProjectionModel();
    expect(model.applyActions(actions(4))).toBe(true);
    expect(model.applyFailure(failure(3))).toBe(false);
    expect(model.failure).toBeNull();

    const newestFailure = failure(5);
    expect(model.applyFailure(newestFailure)).toBe(true);
    expect(model.failure).toBe(newestFailure);
    expect(model.applyActions(actions(4, "Delayed"))).toBe(false);
    expect(model.applyActions(actions(6, "Refreshed"))).toBe(true);
    expect(model.failure).toBeNull();
  });

  it("clears only the exact failure scheduled by its dismissal timer", () => {
    const model = new ExtensionProjectionModel();
    const first = failure(2);
    const second = failure(3);
    expect(model.applyFailure(first)).toBe(true);
    expect(model.applyFailure(second)).toBe(true);
    expect(model.clearFailure(first.projection_revision)).toBe(false);
    expect(model.failure).toBe(second);
    expect(model.clearFailure(second.projection_revision)).toBe(true);
    expect(model.failure).toBeNull();
  });

  it("reveals a failure only in its exact profile and tab context", () => {
    const current = failure(2);
    expect(failureForContext(current, "profile-a", "tab-a")).toBe("tab_discarded");
    expect(failureForContext(current, "profile-b", "tab-a")).toBeNull();
    expect(failureForContext(current, "profile-a", "tab-b")).toBeNull();
    expect(failureForContext(current, null, null)).toBeNull();
  });
});

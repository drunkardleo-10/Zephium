import { describe, expect, it } from "vitest";
import type {
  ExtensionActionFailedView,
  ExtensionActionsView,
  ExtensionManagementView,
  ExtensionRuntimeGrantPromptView,
} from "../src/shared/ipc/bindings";
import {
  ExtensionManagementProjectionModel,
  ExtensionProjectionModel,
  ExtensionRuntimeGrantPromptProjectionModel,
  failureForContext,
  initialExtensionActions,
  initialExtensionManagement,
  initialExtensionRuntimeGrantPrompt,
  managementForProfile,
} from "../src/domain/extensions/extensions-model";
import { ZERO_PROJECTION_REVISION } from "../src/domain/tabs/tabs-model";

function revision(value: number): string {
  return value.toString(16).padStart(32, "0");
}

function grantPrompt(
  value: number,
  requestId = "0000000000000001",
): ExtensionRuntimeGrantPromptView {
  return {
    projection_revision: revision(value),
    prompt: {
      profile_id: "profile-a",
      install_id: "install-a",
      runtime_generation: "0000000000000001",
      request_id: requestId,
      extension_name: "Bitwarden",
      api_permissions: ["clipboardRead"],
      host_permissions: ["https://example.com/*"],
      private_context: false,
      processing: false,
    },
  };
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

function management(value: number, profileId = "profile-a"): ExtensionManagementView {
  return {
    projection_revision: revision(value),
    profile_id: profileId,
    phase: "ready",
    catalog_revision: "0000000000000001",
    entries: [
      {
        install_id: "install-a",
        install_revision: "0000000000000001",
        name: "Bitwarden",
        description: "Password manager",
        author: "Bitwarden Inc.",
        version: "2026.7.0",
        runtime: "active",
        runtime_generation: "0000000000000001",
        grants: {
          initialized: true,
          revision: "0000000000000001",
          api_grants: 3,
          host_grants: 2,
          file_access: false,
          private_access: false,
        },
        compatibility: "degraded",
        limitations: [{ type: "api_permission", name: "webRequest" }],
      },
    ],
    candidates: [],
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

describe("extension management projection admission", () => {
  it("starts unavailable without retaining selectors", () => {
    expect(initialExtensionManagement()).toEqual({
      projection_revision: ZERO_PROJECTION_REVISION,
      profile_id: "",
      phase: "unavailable",
      catalog_revision: null,
      entries: [],
      candidates: [],
    });
  });

  it("replaces the catalog only with a strictly newer management revision", () => {
    const model = new ExtensionManagementProjectionModel();
    const current = management(4);
    expect(model.apply(current)).toBe(true);
    expect(model.management).toBe(current);
    expect(model.apply(management(4, "profile-b"))).toBe(false);
    expect(model.apply(management(3, "profile-b"))).toBe(false);
    expect(model.management).toBe(current);
  });

  it("exposes selectors only for the exact focused profile", () => {
    const current = management(2);
    expect(managementForProfile(current, "profile-a")).toBe(current);
    expect(managementForProfile(current, "profile-b")).toBeNull();
    expect(managementForProfile(current, null)).toBeNull();
  });
});

describe("extension runtime grant projection admission", () => {
  it("starts closed at the zero revision", () => {
    expect(initialExtensionRuntimeGrantPrompt()).toEqual({
      projection_revision: ZERO_PROJECTION_REVISION,
      prompt: null,
    });
  });

  it("accepts only newer exact replacements, including authoritative closure", () => {
    const model = new ExtensionRuntimeGrantPromptProjectionModel();
    const current = grantPrompt(4);
    expect(model.apply(current)).toBe(true);
    expect(model.apply(grantPrompt(4, "0000000000000002"))).toBe(false);
    expect(model.apply(grantPrompt(3, "0000000000000003"))).toBe(false);
    expect(model.view).toBe(current);

    const closed = { projection_revision: revision(5), prompt: null };
    expect(model.apply(closed)).toBe(true);
    expect(model.view).toBe(closed);
  });
});

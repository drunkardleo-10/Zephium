import { describe, expect, it } from "vitest";
import type {
  ExtensionActionFailedView,
  ExtensionActionShortcutView,
  ExtensionActionsView,
  ExtensionManagementAvailabilityChangedView,
  ExtensionManagementView,
  ExtensionRuntimeGrantPromptView,
  OperationDisposition,
} from "../src/shared/ipc/bindings";
import {
  ExtensionDistributionProjectionModel,
  ExtensionActionShortcutProjectionModel,
  ExtensionManagementAvailabilityProjectionModel,
  ExtensionManagementProjectionModel,
  ExtensionProjectionModel,
  ExtensionRuntimeGrantPromptProjectionModel,
  extensionDistributionNotice,
  extensionDistributionRefreshMessage,
  extensionManagementDispositionMessage,
  failureForContext,
  initialExtensionActions,
  initialExtensionManagement,
  initialExtensionManagementAvailability,
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

function shortcut(value: number): ExtensionActionShortcutView {
  return {
    projection_revision: revision(value),
    profile_id: "profile-a",
    tab_id: "tab-a",
    runtime: { install_id: "install-a", generation: "0000000000000001" },
    action_revision: "0000000000000002",
  };
}

function management(value: number, profileId = "profile-a"): ExtensionManagementView {
  return {
    projection_revision: revision(value),
    profile_id: profileId,
    phase: "ready",
    catalog_revision: "0000000000000001",
    profile_policy: {
      revision: "0000000000000001",
      paused: false,
      denied_site_count: 0,
      current_site_available: true,
      current_site_denied: false,
    },
    entries: [
      {
        install_id: "install-a",
        install_revision: "0000000000000001",
        name: "Bitwarden",
        description: "Password manager",
        author: "Bitwarden Inc.",
        version: "2026.7.0",
        has_options_page: true,
        source: "zephium_verified",
        verified_catalog_unix: "1786924800",
        provenance: {
          source_url: "https://example.com/releases/bitwarden",
          upstream_version: "2026.7.0",
          license_expression: "GPL-3.0-only",
          attribution: "Bitwarden contributors",
        },
        runtime: "active",
        runtime_generation: "0000000000000001",
        grants: {
          initialized: true,
          revision: "0000000000000001",
          api_permissions: ["storage", "tabs", "webNavigation"],
          host_permissions: ["http://*/*", "https://*/*"],
          file_access: false,
          private_access: false,
        },
        optional_api: [],
        optional_hosts: [],
        compatibility: "degraded",
        limitations: [{ type: "api_permission", name: "webRequest" }],
      },
    ],
    candidates: [],
    pending_update: null,
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

  it("consumes each action shortcut once and invalidates it with newer context", () => {
    const actionModel = new ExtensionProjectionModel();
    const shortcutModel = new ExtensionActionShortcutProjectionModel();
    expect(actionModel.applyActions(actions(2))).toBe(true);
    expect(shortcutModel.apply(shortcut(3), actionModel.revision)).toBe(true);
    expect(shortcutModel.view).toEqual(shortcut(3));
    expect(shortcutModel.consume(revision(2))).toBe(false);
    expect(shortcutModel.consume(revision(3))).toBe(true);
    expect(shortcutModel.view).toBeNull();
    expect(shortcutModel.consume(revision(3))).toBe(false);

    expect(shortcutModel.apply(shortcut(4), actionModel.revision)).toBe(true);
    expect(actionModel.applyActions(actions(5, "Updated"))).toBe(true);
    shortcutModel.observeContextRevision(actionModel.revision);
    expect(shortcutModel.view).toBeNull();
    expect(shortcutModel.apply(shortcut(4), actionModel.revision)).toBe(false);
  });
});

describe("extension management projection admission", () => {
  it("keeps restart-required distinct from generic activation pending", () => {
    const restart: OperationDisposition = {
      operation_id: "restart",
      outcome: "applied",
      reason: "extension_restart_required",
    };
    expect(extensionManagementDispositionMessage(restart)).toBe(
      "Restart Zephium to activate this extension.",
    );
    expect(
      extensionManagementDispositionMessage({
        ...restart,
        operation_id: "pending",
        reason: "extension_activation_pending",
      }),
    ).toBe("The extension is enabled and will activate when its runtime becomes available.");
  });

  it("keeps the product entry point hidden until a newer availability fact arrives", () => {
    expect(initialExtensionManagementAvailability()).toEqual({
      projection_revision: ZERO_PROJECTION_REVISION,
      availability: "unavailable",
    });
    const model = new ExtensionManagementAvailabilityProjectionModel();
    const configured: ExtensionManagementAvailabilityChangedView = {
      projection_revision: revision(2),
      availability: "configured",
    };
    expect(model.apply(configured)).toBe(true);
    expect(model.view).toBe(configured);
    expect(model.apply({ projection_revision: revision(1), availability: "not_configured" })).toBe(
      false,
    );
    expect(model.view).toBe(configured);
  });

  it("starts unavailable without retaining selectors", () => {
    expect(initialExtensionManagement()).toEqual({
      projection_revision: ZERO_PROJECTION_REVISION,
      profile_id: "",
      phase: "unavailable",
      catalog_revision: null,
      profile_policy: null,
      entries: [],
      candidates: [],
      pending_update: null,
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

  it("rejects contradictory update-review and catalog shapes", () => {
    const model = new ExtensionManagementProjectionModel();
    const contradictory = management(2);
    contradictory.phase = "update_consent_required";
    expect(model.apply(contradictory)).toBe(false);
    expect(model.management).toEqual(initialExtensionManagement());

    const review: ExtensionManagementView = {
      projection_revision: revision(3),
      profile_id: "profile-a",
      phase: "update_consent_required",
      catalog_revision: null,
      profile_policy: null,
      entries: [],
      candidates: [],
      pending_update: {
        review_id: "0000000000000001",
        name: "Example",
        version: "2.0.0",
        source: "zephium_verified",
        verified_catalog_unix: "1786924800",
        provenance: {
          source_url: "https://example.com/extension",
          upstream_version: "2.0.0",
          license_expression: "MIT",
          attribution: "Example",
        },
        added_required_api: ["history"],
        added_required_hosts: [],
        compatibility: "compatible",
        limitations: [],
      },
    };
    expect(model.apply(review)).toBe(true);
    expect(model.management).toBe(review);
  });

  it("exposes selectors only for the exact focused profile", () => {
    const current = management(2);
    expect(managementForProfile(current, "profile-a")).toBe(current);
    expect(managementForProfile(current, "profile-b")).toBeNull();
    expect(managementForProfile(current, null)).toBeNull();
  });
});

describe("extension distribution projection admission", () => {
  it("starts absent and accepts only newer Shell revisions", () => {
    const model = new ExtensionDistributionProjectionModel();
    expect(model.view).toBeNull();

    expect(
      model.apply({
        projection_revision: revision(2),
        state: { phase: "synchronizing" },
      }),
    ).toBe(true);
    expect(model.view?.state.phase).toBe("synchronizing");
    expect(
      model.apply({
        projection_revision: revision(1),
        state: { phase: "idle" },
      }),
    ).toBe(false);
    expect(model.view?.state.phase).toBe("synchronizing");
  });

  it("keeps absent and idle workers silent and exposes only redacted status copy", () => {
    expect(extensionDistributionNotice(null)).toBeNull();
    expect(
      extensionDistributionNotice({
        projection_revision: revision(1),
        state: { phase: "idle" },
      }),
    ).toBeNull();
    expect(
      extensionDistributionNotice({
        projection_revision: revision(2),
        state: { phase: "synchronizing" },
      }),
    ).toEqual({ tone: "progress", message: "Checking for extension updates…" });
    expect(
      extensionDistributionNotice({
        projection_revision: revision(3),
        state: {
          phase: "ready",
          package_count: 1,
          materialized_packages: 1,
          reused_packages: 0,
          exact_retries: 0,
          newly_activated: true,
        },
      }),
    ).toEqual({ tone: "success", message: "Extension updates were installed." });
    expect(
      extensionDistributionNotice({
        projection_revision: revision(4),
        state: {
          phase: "quarantined",
          stage: { type: "catalog_activation" },
          reason: "outcome_unresolved",
        },
      }),
    ).toEqual({
      tone: "warning",
      message: "Extension updates are paused until Zephium restarts.",
    });
  });

  it("keeps immediate refresh admission separate from asynchronous completion", () => {
    expect(extensionDistributionRefreshMessage("accepted")).toBeNull();
    expect(extensionDistributionRefreshMessage("busy")).toBe(
      "An extension update check is already running.",
    );
    expect(extensionDistributionRefreshMessage("quarantined")).toBe(
      "Extension updates are paused until Zephium restarts.",
    );
    expect(extensionDistributionRefreshMessage("unavailable")).toBe(
      "Extension updates are unavailable in this build.",
    );
    expect(extensionDistributionRefreshMessage("shutting_down")).toBe(
      "Extensions are unavailable while Zephium is closing.",
    );
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

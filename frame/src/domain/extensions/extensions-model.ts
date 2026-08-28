import type {
  ExtensionActionFailedView,
  ExtensionActionShortcutView,
  ExtensionActionFailure,
  ExtensionActionsView,
  ExtensionDistributionRefreshAdmissionView,
  ExtensionDistributionView,
  ExtensionManagementAvailabilityChangedView,
  ExtensionManagementView,
  ExtensionRuntimeGrantPromptView,
  OperationDisposition,
} from "../../shared/ipc/bindings";
import { ZERO_PROJECTION_REVISION } from "../tabs/tabs-model";

export function initialExtensionActions(): ExtensionActionsView {
  return {
    projection_revision: ZERO_PROJECTION_REVISION,
    profile_id: "",
    tab_id: null,
    actions: [],
  };
}

export function initialExtensionManagement(): ExtensionManagementView {
  return {
    projection_revision: ZERO_PROJECTION_REVISION,
    profile_id: "",
    phase: "unavailable",
    catalog_revision: null,
    profile_policy: null,
    entries: [],
    candidates: [],
    pending_update: null,
  };
}

export function initialExtensionManagementAvailability(): ExtensionManagementAvailabilityChangedView {
  return {
    projection_revision: ZERO_PROJECTION_REVISION,
    availability: "unavailable",
  };
}

/** Closed user copy for one actor-settled extension management mutation. */
export function extensionManagementDispositionMessage(
  disposition: OperationDisposition,
): string | null {
  if (disposition.outcome === "applied" || disposition.outcome === "no_op") {
    if (disposition.reason === "extension_restart_required") {
      return "Restart Zephium to activate this extension.";
    }
    if (disposition.reason === "extension_activation_pending") {
      return "The extension is enabled and will activate when its runtime becomes available.";
    }
    if (disposition.reason === "extension_enablement_pending") {
      return "The extension was installed, but enabling it is still pending.";
    }
    return null;
  }
  switch (disposition.reason) {
    case "store_conflict":
      return "Extensions changed. Review the refreshed list and try again.";
    case "store_outcome_unknown":
    case "store_reconciliation_failed":
      return "Zephium couldn't verify the extension change. Restart before trying again.";
    case "store_admission_rejected":
      return "Extension management is temporarily unavailable.";
    case "invalid_scope":
      return "This extension can no longer be changed from the current profile.";
    default:
      return "Zephium couldn't apply the extension change.";
  }
}

/** Independent revision admission for the process-immutable product fact. */
export class ExtensionManagementAvailabilityProjectionModel {
  #view: ExtensionManagementAvailabilityChangedView;
  #appliedRevision: string;

  constructor(
    initial: ExtensionManagementAvailabilityChangedView = initialExtensionManagementAvailability(),
  ) {
    this.#view = initial;
    this.#appliedRevision = initial.projection_revision;
  }

  get view(): ExtensionManagementAvailabilityChangedView {
    return this.#view;
  }

  apply(candidate: ExtensionManagementAvailabilityChangedView): boolean {
    if (candidate.projection_revision <= this.#appliedRevision) return false;
    this.#appliedRevision = candidate.projection_revision;
    this.#view = candidate;
    return true;
  }
}

export function initialExtensionRuntimeGrantPrompt(): ExtensionRuntimeGrantPromptView {
  return {
    projection_revision: ZERO_PROJECTION_REVISION,
    prompt: null,
  };
}

/**
 * Framework-free admission for the two actor-ordered extension event streams.
 * Both revisions come from the Shell's one global sequence, so a delayed eval
 * cannot restore stale buttons or surface an obsolete failure after newer
 * extension state has reached chrome.
 */
export class ExtensionProjectionModel {
  #actions: ExtensionActionsView;
  #failure: ExtensionActionFailedView | null = null;
  #appliedRevision: string;

  constructor(initial: ExtensionActionsView = initialExtensionActions()) {
    this.#actions = initial;
    this.#appliedRevision = initial.projection_revision;
  }

  get actions(): ExtensionActionsView {
    return this.#actions;
  }

  get failure(): ExtensionActionFailedView | null {
    return this.#failure;
  }

  get revision(): string {
    return this.#appliedRevision;
  }

  applyActions(candidate: ExtensionActionsView): boolean {
    if (candidate.projection_revision <= this.#appliedRevision) return false;
    this.#appliedRevision = candidate.projection_revision;
    this.#actions = candidate;
    this.#failure = null;
    return true;
  }

  applyFailure(candidate: ExtensionActionFailedView): boolean {
    if (candidate.projection_revision <= this.#appliedRevision) return false;
    this.#appliedRevision = candidate.projection_revision;
    this.#failure = candidate;
    return true;
  }

  clearFailure(revision: string): boolean {
    if (this.#failure?.projection_revision !== revision) return false;
    this.#failure = null;
    return true;
  }
}

/**
 * One-shot admission for a native action shortcut. The shared actor revision
 * floor prevents a delayed shortcut from targeting an action cohort that has
 * already been replaced or failed. Consumption is exact so a Svelte remount
 * cannot replay a prior keyboard gesture.
 */
export class ExtensionActionShortcutProjectionModel {
  #view: ExtensionActionShortcutView | null = null;
  #appliedRevision = ZERO_PROJECTION_REVISION;

  get view(): ExtensionActionShortcutView | null {
    return this.#view;
  }

  observeContextRevision(revision: string): void {
    if (revision <= this.#appliedRevision) return;
    this.#appliedRevision = revision;
    if (this.#view !== null && this.#view.projection_revision <= revision) this.#view = null;
  }

  apply(candidate: ExtensionActionShortcutView, contextRevision: string): boolean {
    this.observeContextRevision(contextRevision);
    if (candidate.projection_revision <= this.#appliedRevision) return false;
    this.#appliedRevision = candidate.projection_revision;
    this.#view = candidate;
    return true;
  }

  consume(revision: string): boolean {
    if (this.#view?.projection_revision !== revision) return false;
    this.#view = null;
    return true;
  }

  clear(): void {
    this.#view = null;
  }
}

/**
 * Revision admission for the lazy installed-extension catalog. This stream is
 * intentionally independent from toolbar actions: both use the Shell's global
 * sequence, but skipping an older event from a different semantic stream could
 * otherwise preserve an even older catalog in chrome.
 */
export class ExtensionManagementProjectionModel {
  #management: ExtensionManagementView;
  #appliedRevision: string;

  constructor(initial: ExtensionManagementView = initialExtensionManagement()) {
    this.#management = initial;
    this.#appliedRevision = initial.projection_revision;
  }

  get management(): ExtensionManagementView {
    return this.#management;
  }

  apply(candidate: ExtensionManagementView): boolean {
    if (
      candidate.projection_revision <= this.#appliedRevision ||
      !validExtensionManagementShape(candidate)
    )
      return false;
    this.#appliedRevision = candidate.projection_revision;
    this.#management = candidate;
    return true;
  }
}

function validExtensionManagementShape(candidate: ExtensionManagementView): boolean {
  if (candidate.phase === "ready") {
    return (
      candidate.catalog_revision !== null &&
      candidate.profile_policy !== null &&
      candidate.pending_update === null
    );
  }
  if (candidate.phase === "update_consent_required") {
    return (
      candidate.catalog_revision === null &&
      candidate.profile_policy === null &&
      candidate.entries.length === 0 &&
      candidate.candidates.length === 0 &&
      candidate.pending_update !== null &&
      candidate.pending_update.added_required_api.length +
        candidate.pending_update.added_required_hosts.length +
        candidate.pending_update.limitations.length >
        0
    );
  }
  return (
    candidate.catalog_revision === null &&
    candidate.profile_policy === null &&
    candidate.entries.length === 0 &&
    candidate.candidates.length === 0 &&
    candidate.pending_update === null
  );
}

/**
 * Independent revision admission for the optional product distribution
 * worker. `null` is meaningful: ordinary builds do not construct the worker
 * and must not imply that an update channel is idle or available.
 */
export class ExtensionDistributionProjectionModel {
  #view: ExtensionDistributionView | null = null;
  #appliedRevision = ZERO_PROJECTION_REVISION;

  get view(): ExtensionDistributionView | null {
    return this.#view;
  }

  apply(candidate: ExtensionDistributionView): boolean {
    if (candidate.projection_revision <= this.#appliedRevision) return false;
    this.#appliedRevision = candidate.projection_revision;
    this.#view = candidate;
    return true;
  }
}

export type ExtensionDistributionNotice = {
  tone: "progress" | "success" | "warning";
  message: string;
};

/**
 * Maps the closed, redacted worker state to equally redacted product copy.
 * Idle and shutdown are deliberately silent: an ordinary or unavailable
 * updater must not look like a successful update channel.
 */
export function extensionDistributionNotice(
  view: ExtensionDistributionView | null,
): ExtensionDistributionNotice | null {
  if (view === null) return null;
  switch (view.state.phase) {
    case "idle":
    case "shutdown":
      return null;
    case "synchronizing":
      return { tone: "progress", message: "Checking for extension updates…" };
    case "ready":
      return {
        tone: "success",
        message: view.state.newly_activated
          ? "Extension updates were installed."
          : "Extensions are up to date.",
      };
    case "failed":
      return {
        tone: "warning",
        message: "Extension updates could not be checked. Installed extensions were not changed.",
      };
    case "quarantined":
      return {
        tone: "warning",
        message: "Extension updates are paused until Zephium restarts.",
      };
  }
}

/**
 * Maps only immediate refresh admission. Completion remains authoritative in
 * the independently revisioned distribution projection above.
 */
export function extensionDistributionRefreshMessage(
  admission: ExtensionDistributionRefreshAdmissionView,
): string | null {
  switch (admission) {
    case "accepted":
      return null;
    case "busy":
      return "An extension update check is already running.";
    case "quarantined":
      return "Extension updates are paused until Zephium restarts.";
    case "unavailable":
      return "Extension updates are unavailable in this build.";
    case "shutting_down":
      return "Extensions are unavailable while Zephium is closing.";
  }
}

/**
 * Independent revision admission for the process-wide native permission
 * prompt. A replacement carrying `null` is authoritative closure; retaining
 * the old prompt after that event would leave stale consent controls live.
 */
export class ExtensionRuntimeGrantPromptProjectionModel {
  #view: ExtensionRuntimeGrantPromptView;
  #appliedRevision: string;

  constructor(initial: ExtensionRuntimeGrantPromptView = initialExtensionRuntimeGrantPrompt()) {
    this.#view = initial;
    this.#appliedRevision = initial.projection_revision;
  }

  get view(): ExtensionRuntimeGrantPromptView {
    return this.#view;
  }

  apply(candidate: ExtensionRuntimeGrantPromptView): boolean {
    if (candidate.projection_revision <= this.#appliedRevision) return false;
    this.#appliedRevision = candidate.projection_revision;
    this.#view = candidate;
    return true;
  }
}

export function managementForProfile(
  management: ExtensionManagementView,
  profileId: string | null,
): ExtensionManagementView | null {
  return profileId !== null && management.profile_id === profileId ? management : null;
}

export function failureForContext(
  failure: Pick<ExtensionActionFailedView, "profile_id" | "tab_id" | "reason"> | null,
  profileId: string | null,
  tabId: string | null,
): ExtensionActionFailure | null {
  return failure !== null && failure.profile_id === profileId && failure.tab_id === tabId
    ? failure.reason
    : null;
}

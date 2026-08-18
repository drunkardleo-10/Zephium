import type {
  ExtensionActionFailedView,
  ExtensionActionFailure,
  ExtensionActionsView,
  ExtensionDistributionRefreshAdmissionView,
  ExtensionDistributionView,
  ExtensionManagementAvailabilityChangedView,
  ExtensionManagementView,
  ExtensionRuntimeGrantPromptView,
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
    entries: [],
    candidates: [],
  };
}

export function initialExtensionManagementAvailability(): ExtensionManagementAvailabilityChangedView {
  return {
    projection_revision: ZERO_PROJECTION_REVISION,
    availability: "unavailable",
  };
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
    if (candidate.projection_revision <= this.#appliedRevision) return false;
    this.#appliedRevision = candidate.projection_revision;
    this.#management = candidate;
    return true;
  }
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

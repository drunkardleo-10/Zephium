import type {
  ExtensionActionFailedView,
  ExtensionActionFailure,
  ExtensionActionsView,
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

export function failureForContext(
  failure: Pick<ExtensionActionFailedView, "profile_id" | "tab_id" | "reason"> | null,
  profileId: string | null,
  tabId: string | null,
): ExtensionActionFailure | null {
  return failure !== null && failure.profile_id === profileId && failure.tab_id === tabId
    ? failure.reason
    : null;
}

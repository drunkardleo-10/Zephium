import type {
  ExtensionActionFailedView,
  ExtensionActionShortcutView,
  ExtensionActionFailure,
  ExtensionActionsView,
} from "$shared/ipc/bindings";
import { ZERO_PROJECTION_REVISION } from "$domain/tabs";

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

export function failureForContext(
  failure: Pick<ExtensionActionFailedView, "profile_id" | "tab_id" | "reason"> | null,
  profileId: string | null,
  tabId: string | null,
): ExtensionActionFailure | null {
  return failure !== null && failure.profile_id === profileId && failure.tab_id === tabId
    ? failure.reason
    : null;
}

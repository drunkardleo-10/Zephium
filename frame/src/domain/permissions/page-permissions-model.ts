import type { PagePermissionPromptView } from "../../shared/ipc/bindings";
import { ZERO_PROJECTION_REVISION } from "../tabs/tabs-model";

export function initialPagePermissionPrompt(): PagePermissionPromptView {
  return {
    projection_revision: ZERO_PROJECTION_REVISION,
    prompt: null,
  };
}

/**
 * Independent exact-replacement admission for browser-owned page consent.
 * A newer `null` closes stale controls; frontend timing can never revive an
 * older native request after navigation or native timeout.
 */
export class PagePermissionPromptProjectionModel {
  #view: PagePermissionPromptView;
  #appliedRevision: string;

  constructor(initial: PagePermissionPromptView = initialPagePermissionPrompt()) {
    this.#view = initial;
    this.#appliedRevision = initial.projection_revision;
  }

  get view(): PagePermissionPromptView {
    return this.#view;
  }

  apply(candidate: PagePermissionPromptView): boolean {
    if (candidate.projection_revision <= this.#appliedRevision) return false;
    this.#appliedRevision = candidate.projection_revision;
    this.#view = candidate;
    return true;
  }
}

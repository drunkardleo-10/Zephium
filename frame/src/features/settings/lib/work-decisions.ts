import * as m from "$shared/i18n/messages";
import type { WorkDecisionChoiceV1 } from "$shared/ipc/bindings";

export const decisionChoices: readonly WorkDecisionChoiceV1[] = ["recommended", "standard", "off"];

const LABELS: Record<WorkDecisionChoiceV1, () => string> = {
  recommended: m.settings_decisions_recommended,
  standard: m.settings_decisions_standard,
  off: m.settings_decisions_off,
};

const DETAILS: Record<WorkDecisionChoiceV1, () => string> = {
  recommended: m.settings_decisions_recommended_help,
  standard: m.settings_decisions_standard_help,
  off: m.settings_decisions_off_help,
};

export const decisionLabel = (choice: WorkDecisionChoiceV1) => LABELS[choice]();
export const decisionDetail = (choice: WorkDecisionChoiceV1) => DETAILS[choice]();

/**
 * Names the providers that receive page text under what actually runs. Only
 * an effective `recommended` reaches TypeSafe; every mode still reaches OpenAI.
 */
export function decisionDisclosure(effective: WorkDecisionChoiceV1): string {
  switch (effective) {
    case "recommended":
      return m.settings_decisions_disclosure_typesafe();
    case "standard":
    case "off":
      return m.settings_decisions_disclosure_openai();
  }
}

/** Shown only when Rust runs something other than the stored choice. */
export const decisionActive = (choice: WorkDecisionChoiceV1, effective: WorkDecisionChoiceV1) =>
  choice === effective ? null : m.settings_decisions_active({ mode: decisionLabel(effective) });

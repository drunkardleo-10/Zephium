import type { WorkHumanPageV1, WorkHumanPhaseV1, WorkHumanReasonV1 } from "$shared/ipc/bindings";
import * as m from "$shared/i18n/messages";

/** Display-only view of one held page; the wire projection stays in the domain. */
export type HumanPage = {
  attempt: string;
  step: string;
  generation: number;
  phase: WorkHumanPhaseV1;
  reason: WorkHumanReasonV1;
  remaining: number;
  canContinue: boolean;
};

const REASONS: Record<WorkHumanReasonV1, () => string> = {
  sign_in: m.work_human_sign_in,
  challenge: m.work_human_challenge,
  permission: m.work_human_permission,
  verification: m.work_human_verification,
  user_decision: m.work_human_user_decision,
  sensitive_effect: m.work_human_sensitive_effect,
  unsupported_interaction: m.work_human_unsupported_interaction,
};

/** The badge on a waiting card: why a person is needed, in plain words. */
export const reasonBadge = (reason: WorkHumanReasonV1) => REASONS[reason]();

/** What a card says once the page is no longer merely waiting. */
export function phaseLabel(phase: WorkHumanPhaseV1): string | null {
  switch (phase) {
    case "presenting":
      return m.work_human_presenting();
    case "presented":
      return m.work_human_presented();
    case "continuing":
      return m.work_human_continuing();
    default:
      return null;
  }
}

/** Whole seconds, rounded up, so the last second is shown rather than skipped. */
function secondsLeft(remaining: number): number | null {
  if (!Number.isFinite(remaining) || remaining <= 0) return null;
  return Math.max(1, Math.ceil(remaining / 1000));
}

const NEAR = 60_000;

/** The card is quiet about a long wait: the countdown only appears near the end. */
export function cardCountdown(remaining: number): string | null {
  const seconds = secondsLeft(remaining);
  return seconds !== null && remaining < NEAR ? m.work_human_seconds_left({ seconds }) : null;
}

/** Still held for a person: `reading` and `released` are the agent's again. */
const HELD: readonly WorkHumanPhaseV1[] = [
  "waiting_for_human",
  "presenting",
  "presented",
  "continuing",
];
const RANK = (phase: WorkHumanPhaseV1) => HELD.indexOf(phase);

/** One card shows one state: the page furthest into a takeover wins. */
export function heldPage(pages: readonly WorkHumanPageV1[]): WorkHumanPageV1 | null {
  let best: WorkHumanPageV1 | null = null;
  for (const page of pages) {
    if (RANK(page.phase) < 0) continue;
    if (!best || RANK(page.phase) > RANK(best.phase)) best = page;
  }
  return best;
}

export const humanPage = (page: WorkHumanPageV1): HumanPage => ({
  attempt: page.id.attempt,
  step: page.id.step,
  generation: page.id.generation,
  phase: page.phase,
  reason: page.reason,
  remaining: page.remaining_millis,
  canContinue: page.can_continue,
});

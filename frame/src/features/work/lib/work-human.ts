import type {
  WorkExecutionFact,
  WorkHumanPageV1,
  WorkHumanPhaseV1,
  WorkHumanReasonV1,
  WorkHumanRegionV1,
} from "$shared/ipc/bindings";
import type { WorkHumanFailure } from "$domain/work-human";
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

/** The short label on a waiting card: what the person is asked for. */
export const reasonBadge = (reason: WorkHumanReasonV1) => REASONS[reason]();

/** Why the page waits, in one sentence, person first: the card, the line and the pane say it alike. */
export function reasonSentence(reason: WorkHumanReasonV1, host: string): string {
  switch (reason) {
    case "sign_in":
      return m.work_human_why_sign_in({ host });
    case "challenge":
      return m.work_human_why_challenge({ host });
    case "permission":
      return m.work_human_why_permission({ host });
    case "verification":
      return m.work_human_why_verification({ host });
    case "user_decision":
      return m.work_human_why_user_decision();
    case "sensitive_effect":
      return m.work_human_why_sensitive_effect({ host });
    case "unsupported_interaction":
      return m.work_human_why_unsupported_interaction();
  }
}

/** Why the loop refused a signed-in step, by the closed names Rust gives them. */
export type AccountRefusal =
  { kind: "AccountWrite"; host: string } | { kind: "PageBudget"; host: string; pages: number };

/** A signed-in refusal on the agent line, as a plain sentence. */
export function accountRefusalSentence(refusal: AccountRefusal): string {
  switch (refusal.kind) {
    case "AccountWrite":
      return m.work_line_account_write({ host: refusal.host });
    case "PageBudget":
      return m.work_line_page_budget({ host: refusal.host, pages: refusal.pages });
  }
}

/** The refusal a run's projection shows: a granted origin whose pages are all used. */
export function accountRefusal(execution: WorkExecutionFact): AccountRefusal | null {
  const spent = (execution.accounts ?? []).find(
    (use) => use.pages > 0 && use.pages_used >= use.pages,
  );
  return spent ? { kind: "PageBudget", host: spent.host, pages: spent.pages } : null;
}

/** A refused command is one line in the pane, in the person's words. */
export const failureLine = (failure: WorkHumanFailure) =>
  failure === "not_found" || failure === "conflict"
    ? m.work_human_gone()
    : m.work_human_unavailable();

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

/** How long the person still has; the pane always shows it. */
export function countdownLabel(remaining: number): string | null {
  const seconds = secondsLeft(remaining);
  return seconds === null ? null : m.work_human_seconds_left({ seconds });
}

/** The card is quiet about a long wait: the countdown only appears near the end. */
export const cardCountdown = (remaining: number) =>
  remaining < NEAR ? countdownLabel(remaining) : null;

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

/** Rust floors the size only; the corner may sit at the content view's origin. */
export const MIN_REGION = 64;
const MAX_EXTENT = 8192;

/**
 * The native view is placed the way a Work pane hole is: the measured DOM box
 * is already in the window's logical points. Integers only, so a sub-pixel
 * reflow never re-presents the same region.
 */
export function regionOf(
  box: { x: number; y: number; width: number; height: number },
  parent: { width: number; height: number },
): WorkHumanRegionV1 | null {
  const x = Math.round(box.x);
  const y = Math.round(box.y);
  const width = Math.round(box.width);
  const height = Math.round(box.height);
  if (![x, y, width, height, parent.width, parent.height].every(Number.isFinite)) return null;
  if (x < 0 || y < 0) return null;
  if (width < MIN_REGION || height < MIN_REGION) return null;
  if (x + width > MAX_EXTENT || y + height > MAX_EXTENT) return null;
  if (x + width > Math.floor(parent.width) || y + height > Math.floor(parent.height)) return null;
  return { x, y, width, height };
}

/** Two regions are the same placement, so nothing is torn down to rebuild it. */
export const sameRegion = (a: WorkHumanRegionV1 | null, b: WorkHumanRegionV1 | null) =>
  !!a && !!b && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;

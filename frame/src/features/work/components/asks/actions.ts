import type { WorkSession } from "$domain/work";
import type { WorkHumanSession } from "$domain/work-human";
import type { WorkHumanPageV1 } from "$shared/ipc/bindings";
import { askPart, asksOf, openAsks } from "./asks";

export type ConfirmDecision = "approve" | "allow_run" | "decline";

/** What an ask card can do; the owner binds it to the run it shows. */
export type AskActions = {
  confirm(step: string, decision: ConfirmDecision): Promise<boolean>;
  answer(step: string, text: string): Promise<boolean>;
  /** Presents a held page for the person: sign in, or look at what will be sent. */
  openPage?(step: string): void;
  /** The person says they signed in: the held page carries on from where it waits. */
  signedIn?(page: WorkHumanPageV1): Promise<boolean>;
};

/** The run's own operations: Confirm steps through ApproveStep, questions through AnswerStep. */
export function runActions(
  session: WorkSession,
  execution: string,
  human?: { session: WorkHumanSession; work: string },
  openPage?: (step: string) => void,
): AskActions {
  return {
    confirm: (step, decision) =>
      session.execute({
        kind: "approve_step",
        execution,
        step,
        approve: decision !== "decline",
        ...(decision === "allow_run" ? { for_run: true } : {}),
      }),
    answer: (step, text) => session.answerStep(execution, step, text),
    ...(openPage ? { openPage } : {}),
    ...(human
      ? {
          signedIn: async (page: WorkHumanPageV1) =>
            (await human.session.continue(human.work, page.id, "anonymous")) === null,
        }
      : {}),
  };
}

/**
 * The open asks of a work's live run, as the canvas's part rows take them
 * (`StageOptions.asks`): which part, the card, and its props.
 */
export function partAsks(
  session: WorkSession,
  human?: { session: WorkHumanSession; work: string },
  openPage?: (step: string) => void,
): { part: string; view: string; props: Record<string, unknown> }[] {
  const execution = session.projection?.executions.at(-1);
  if (!execution) return [];
  const held = human ? (human.session.pages.get(human.work) ?? []) : [];
  const actions = runActions(session, execution.id, human, openPage);
  return openAsks(asksOf(execution, session.pages, held)).flatMap((ask) => {
    const part = askPart(ask);
    return part ? [{ part, view: "AskCard", props: { ask, actions, placement: "canvas" } }] : [];
  });
}

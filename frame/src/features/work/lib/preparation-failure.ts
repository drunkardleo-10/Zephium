import type { WorkOperationStateV1 } from "$shared/ipc/bindings";
/** Presentation of the actual native preparation outcome, including refusal before a draft exists. */
export function preparationFailure(state: WorkOperationStateV1 | undefined): string | null {
  if (state?.kind === "refused") return state.error;
  if (state?.kind === "settled")
    return state.response.reply.kind === "error" ? state.response.reply.error : null;
  if (state?.kind !== "planned") return null;
  const outcome = state.response.outcome;
  if (outcome.kind === "refused") return outcome.reason.kind;
  return outcome.response.reply.kind === "error" ? outcome.response.reply.error : null;
}

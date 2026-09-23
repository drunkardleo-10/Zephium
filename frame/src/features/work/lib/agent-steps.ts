import type { WorkExecutionFact, WorkRuntimeProjection } from "$domain/work";

/** Whether an execution is still going: not settled and not left by an earlier launch. */
export function isLive(state: WorkRuntimeProjection, execution: WorkExecutionFact): boolean {
  return (
    ["approved", "running", "cancel_requested"].includes(execution.status) &&
    !state.interrupted.includes(execution.id)
  );
}

/**
 * The agent's latest line for people, from the most recent turn that said
 * something. The finishing turn says nothing on its turn step; its closing
 * sentence rides the finish step, so a settled run ends on that line.
 */
export function agentLine(execution: WorkExecutionFact): string | null {
  const steps = execution.steps ?? [];
  for (let index = steps.length - 1; index >= 0; index--) {
    const step = steps[index]!;
    if ((step.kind.kind === "turn" || step.kind.kind === "finish") && step.note) return step.note;
  }
  return null;
}

/** Steps that work inside a granted folder; each settles onto a file record. */
export const FILE_STEPS = ["list", "read_file", "search_files", "write_file", "edit_file"];

/** Whether an execution ran under the routine agent grant. */
export function isAgentExecution(execution: WorkExecutionFact): boolean {
  return execution.spec.nodes.some((node) => node.capability.kind === "agent");
}

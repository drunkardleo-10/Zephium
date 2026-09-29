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

/**
 * Why a run that did not finish ended, in Rust's words: the note of the last
 * settled step that did not succeed. Null when no such step said anything.
 */
export function endingNote(execution: WorkExecutionFact): string | null {
  const steps = execution.steps ?? [];
  for (let index = steps.length - 1; index >= 0; index--) {
    const step = steps[index]!;
    const note = step.note?.trim();
    if (note && step.status !== "succeeded" && step.status !== "running") return note;
  }
  return null;
}

/** Steps that work inside a granted folder; each settles onto a file record. */
export const FILE_STEPS = ["list", "read_file", "search_files", "write_file", "edit_file"];

/** Whether a run is the lead's: its parts, inputs and objects are facts it recorded itself. */
export function leadRun(execution: WorkExecutionFact): boolean {
  // Runs recorded before the grant was carried have none.
  return execution.spec.nodes.some(
    (node) => node.capability.kind === "agent" && !!node.capability.grant?.lead,
  );
}

/** Whether an execution ran under the routine agent grant. */
export function isAgentExecution(execution: WorkExecutionFact): boolean {
  return execution.spec.nodes.some((node) => node.capability.kind === "agent");
}

/** What the agent acts on now, from the steps it is running; the canvas stands it there. */
export type AgentDoing = "thinking" | "searching" | "reading" | "working" | "writing" | "done";
export function agentDoing(execution: WorkExecutionFact): AgentDoing {
  const steps = execution.steps ?? [];
  if (steps.some((step) => step.kind.kind === "finish" && step.status === "succeeded"))
    return "done";
  const running = new Set<string>(
    steps.flatMap((step) => (step.status === "running" ? [step.kind.kind] : [])),
  );
  if (running.has("publish") || running.has("finish")) return "writing";
  if (running.has("read") || running.has("discover")) return "reading";
  if (running.has("run_command") || FILE_STEPS.some((kind) => running.has(kind))) return "working";
  if (running.has("search")) return "searching";
  return "thinking";
}

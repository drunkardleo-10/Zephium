import type { WorkCommandRecordV1, WorkExecutionFact, WorkStepFact } from "$shared/ipc/bindings";
import * as m from "$shared/i18n/messages";

/** Only a command explicitly admitted for a person decision opens a review. */
export function pendingCommand(execution: WorkExecutionFact): WorkStepFact | undefined {
  if (execution.status !== "running") return undefined;
  return execution.steps?.find(
    (step) =>
      step.kind.kind === "run_command" &&
      step.status === "running" &&
      step.kind.decision == null &&
      step.local?.policy != null &&
      step.local.policy.scope !== "none",
  );
}

export function commandClassLabel(step: WorkStepFact): string {
  const policy = step.local?.policy;
  if (!policy) return "";
  if (step.kind.kind === "run_command" && policy.scope !== "none" && step.kind.decision !== true)
    return step.kind.decision === false ? m.work_env_file_declined() : m.work_local_review();
  if (policy.class === "read") return m.work_local_ran_without_asking();
  if (policy.class === "write") return m.work_local_allowed_folder();
  return m.work_local_you_approved();
}

export function commandSummary(record: WorkCommandRecordV1): string {
  const result = record.command;
  const duration = (result.elapsed_ms / 1000).toFixed(1);
  if (result.signal != null)
    return m.work_local_signal_summary({ signal: String(result.signal), duration });
  if (result.exit != null)
    return m.work_local_exit_summary({ exit: String(result.exit), duration });
  return m.work_local_stopped_summary({ duration });
}

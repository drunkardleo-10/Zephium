import type { WorkRuntimeProjection, WorkSignalV1 } from "$shared/ipc/bindings";

/** Signals describe activity only while their entire durable join is current. */
export function currentActivity(state: WorkRuntimeProjection, signals: readonly WorkSignalV1[]) {
  const seen = new Set<string>();
  return signals.filter((signal) => {
    if (seen.has(signal.attempt)) return false;
    const execution = state.executions.find((entry) => entry.id === signal.execution);
    const current =
      signal.version === 1 &&
      signal.profile === state.work.profile &&
      signal.work === state.work.id &&
      signal.basis_revision === state.work.revision &&
      !state.interrupted.includes(signal.execution) &&
      state.owners?.some(
        (entry) => entry.execution === signal.execution && entry.owner === signal.owner,
      ) &&
      execution &&
      ["running", "cancel_requested"].includes(execution.status) &&
      execution.attempts.some(
        (attempt) =>
          attempt.id === signal.attempt &&
          attempt.node === signal.node &&
          attempt.status === "running",
      );
    if (current) seen.add(signal.attempt);
    return current;
  });
}

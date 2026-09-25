import { observe } from "$shared/lib/observe";
import type { OperationAdmission, OperationDisposition } from "$shared/ipc/bindings";
import { waitForDisposition } from "./operations";

export type Settlement =
  | {
      outcome: "applied" | "no_op" | "deferred" | "rejected";
      disposition: OperationDisposition | null;
    }
  | {
      outcome: "failed";
      reason: "transport" | "timeout" | "unresolved" | "observation_aborted";
      operation_id?: string;
    };

/** Actor disposition only. Deferred callers still await their authoritative projection. */
export async function settle(
  admission: Promise<OperationAdmission>,
  timeoutMs = 5000,
  signal?: AbortSignal,
): Promise<Settlement> {
  const deadline = performance.now() + timeoutMs;
  const admitted = await observe(admission, timeoutMs, signal);
  if (admitted.state !== "received")
    return {
      outcome: "failed",
      reason:
        admitted.state === "aborted"
          ? "observation_aborted"
          : admitted.state === "failed"
            ? "transport"
            : "timeout",
    };
  if (!admitted.value.accepted) return { outcome: "rejected", disposition: null };
  const operationId = admitted.value.operation_id;
  // Only already-applied UI-only actions may omit a native operation identity.
  if (!operationId) return { outcome: "applied", disposition: null };
  const remaining = deadline - performance.now();
  const result = await observe(
    waitForDisposition(operationId, Math.max(0, remaining), signal),
    remaining,
    signal,
  );
  if (result.state !== "received")
    return {
      outcome: "failed",
      operation_id: operationId,
      reason:
        result.state === "aborted"
          ? "observation_aborted"
          : result.state === "failed"
            ? "transport"
            : "timeout",
    };
  if (result.value.state !== "processed" || result.value.disposition.operation_id !== operationId)
    return { outcome: "failed", reason: "unresolved", operation_id: operationId };
  const disposition = result.value.disposition;
  return {
    outcome: disposition.outcome === "native_admission_failed" ? "rejected" : disposition.outcome,
    disposition,
  };
}

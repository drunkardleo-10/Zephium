import type {
  WorkRuntimeProjection,
  WorkPlanRevision,
  WorkAgentGrantV1,
  WorkExecutionLimits,
} from "$shared/ipc/bindings";

/** The routine public envelope shown in the composer; Rust enforces it. */
export const AGENT_GRANT: WorkAgentGrantV1 = {
  provider: "open_ai",
  model: "gpt-5.6-luna",
  max_turns: 10,
  max_steps: 32,
  browse_hops: 4,
};
export const AGENT_LIMITS: WorkExecutionLimits = {
  model_tokens: 1_000_000,
  cost_micro_usd: 1_500_000,
  operations: 64,
  timeout_seconds: 1800,
  max_workers: 2,
};

const identity = (value: string) => /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/u.test(value);
export function validRevision(value: string): boolean {
  return /^[1-9][0-9]{0,19}$/u.test(value) && BigInt(value) <= 9_223_372_036_854_775_807n;
}
export function newerRevision(next: string, previous: string): boolean {
  return validRevision(next) && validRevision(previous) && BigInt(next) > BigInt(previous);
}

/** Correlation only. Rust mints every durable entity and all execution authority. */
export function commandId(): string {
  let bits = BigInt(Date.now());
  for (const byte of crypto.getRandomValues(new Uint8Array(10))) bits = (bits << 8n) | BigInt(byte);
  const alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
  let result = "";
  for (let i = 0; i < 26; i++) {
    result = alphabet[Number(bits & 31n)] + result;
    bits >>= 5n;
  }
  return result;
}

/** Transport admission, not an alternative domain validator or scheduler. */
export function admitProjection(
  projection: WorkRuntimeProjection,
  profile: string,
  work: string,
  current: WorkRuntimeProjection | null,
): boolean {
  if (
    projection.version !== 1 ||
    projection.work.schema_version !== 2 ||
    projection.work.profile !== profile ||
    projection.work.id !== work ||
    !identity(work) ||
    !validRevision(projection.work.revision) ||
    projection.executions.length > 16 ||
    projection.work.questions.length > 32 ||
    (projection.work.plan?.draft.nodes.length ?? 0) > 64
  )
    return false;
  return (
    !current ||
    current.work.id !== work ||
    projection.work.revision === current.work.revision ||
    newerRevision(projection.work.revision, current.work.revision)
  );
}

export function planProposal(plan: WorkPlanRevision) {
  return {
    nodes: plan.draft.nodes.map((node, key) => ({
      key,
      objective: node.objective,
      outputs: node.outputs.map((output) => ({ ...output })),
      dependencies: node.dependencies.map((id) =>
        plan.draft.nodes.findIndex((item) => item.id === id),
      ),
    })),
  };
}

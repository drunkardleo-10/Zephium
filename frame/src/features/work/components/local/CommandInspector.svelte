<script lang="ts">
  import type { WorkStepFact } from "$shared/ipc/bindings";
  import type { WorkSession } from "$domain/work";
  import { pendingCommand, commandClassLabel } from "../../lib/local-steps";
  import CommandReview from "./CommandReview.svelte";
  import CommandRecord from "./CommandRecord.svelte";
  let {
    step,
    session,
    ondecided,
  }: {
    step: WorkStepFact;
    session: WorkSession;
    ondecided?: () => void;
  } = $props();
  const execution = $derived(
    session.projection?.executions.find((entry) =>
      entry.steps?.some((item) => item.id === step.id),
    ),
  );
  const waiting = $derived(execution && pendingCommand(execution)?.id === step.id);
  const record = $derived(execution?.command_evidence?.find((entry) => entry.id === step.evidence));
  async function decide(approve: boolean) {
    if (!waiting || session.pending) return;
    if (!(await session.approveStep(step.id, approve))) throw new Error("Decision was not saved");
    ondecided?.();
  }
</script>

{#if waiting}
  <CommandReview {step} disabled={!!session.pending} ondecision={decide} />
{:else if record}
  <p>{commandClassLabel(step)}</p>
  <CommandRecord {record} />
{:else}
  <p>{step.note ?? ""}</p>
  {#if step.local?.output}<pre>{step.local.output.text}</pre>{/if}
{/if}

<style>
  p {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  pre {
    overflow: auto;
    max-block-size: 360px;
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>

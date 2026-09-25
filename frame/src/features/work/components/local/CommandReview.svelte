<script lang="ts">
  import type { WorkStepFact } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  import { homePath } from "../../lib/work-files";
  let {
    step,
    disabled = false,
    ondecision,
  }: {
    step: WorkStepFact;
    disabled?: boolean;
    ondecision: (approve: boolean) => Promise<void>;
  } = $props();
  const command = $derived(step.kind.kind === "run_command" ? step.kind : undefined);
  const policy = $derived(step.local?.policy);
  const reasons = {
    inspection: m.work_local_reason_inspection,
    project_execution: m.work_local_reason_project_execution,
    file_change: m.work_local_reason_file_change,
    unknown_program: m.work_local_reason_unknown_program,
    network: m.work_local_reason_network,
    destructive: m.work_local_reason_destructive,
    privilege: m.work_local_reason_privilege,
    outside_roots: m.work_local_reason_outside_roots,
    shell_syntax: m.work_local_reason_shell_syntax,
  };
  let busy = $state(false);
  let failed = $state(false);
  async function decide(approve: boolean) {
    if (busy || disabled) return;
    busy = true;
    failed = false;
    try {
      await ondecision(approve);
    } catch {
      failed = true;
    } finally {
      busy = false;
    }
  }
</script>

{#if command && policy}
  <section aria-label={m.work_local_review()}>
    <p class="path">{homePath(command.cwd)}</p>
    <pre>{command.command}</pre>
    <p>{reasons[policy.reason]()}</p>
    {#if policy.scope === "folder"}
      <p>{m.work_local_folder_scope({ path: homePath(policy.root) })}</p>
      <p>{m.work_local_approval_boundary()}</p>
    {/if}
    {#if failed}<p role="alert">{m.work_local_approval_failed()}</p>{/if}
    <div class="actions">
      <Button variant="primary" disabled={disabled || busy} onclick={() => void decide(true)}
        >{m.work_line_approve_change()}</Button
      >
      <Button disabled={disabled || busy} onclick={() => void decide(false)}
        >{m.work_line_reject_change()}</Button
      >
    </div>
  </section>
{/if}

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-inline-size: 0;
  }

  p {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .path {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }

  pre {
    margin: 0;
    padding: 12px;
    border-radius: var(--radius-row);
    color: var(--color-text);
    background: var(--color-fill);
    font-family: var(--font-mono);
    font-size: var(--text-label);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    gap: 8px;
  }
</style>

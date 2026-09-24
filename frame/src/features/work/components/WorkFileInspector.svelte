<script lang="ts">
  import type { WorkFileEvidenceV1 } from "$shared/ipc/bindings";
  import type { WorkSession } from "$domain/work";
  import Button from "$shared/ui/Button";
  import { fileName, homePath } from "../lib/work-files";
  import * as m from "$shared/i18n/messages";
  let {
    file,
    proposal,
    onback,
    onreveal,
    ondecided,
  }: {
    /** What one settled file step disclosed: a listing, an excerpt, hits, or a diff. */
    file?: WorkFileEvidenceV1;
    /** A change a run is proposing, with the session that can answer it. */
    proposal?: { session: WorkSession; step: string };
    onback?: () => void;
    /** Shows the file where it lives; silent when the application declines. */
    onreveal?: (path: string) => void;
    /** The person approved or declined; the lift closes on their word. */
    ondecided?: () => void;
  } = $props();
  const labels: Record<string, () => string> = {
    directory: m.work_env_file_listing,
    search: m.work_env_file_search,
    written: m.work_env_file_written,
  };
  const session = $derived(proposal?.session);
  const step = $derived.by(() => {
    const wanted = proposal?.step;
    if (!wanted) return undefined;
    for (const execution of session?.projection?.executions ?? [])
      for (const entry of execution.steps ?? []) if (entry.id === wanted) return entry;
    return undefined;
  });
  const change = $derived.by(() => {
    const kind = step?.kind;
    switch (kind?.kind) {
      case "write_file":
      case "edit_file":
      case "move_file":
      case "delete_file":
      case "run_command":
        return kind;
      default:
        return undefined;
    }
  });
  const waiting = $derived(
    !!change &&
      step?.status === "running" &&
      (change.decision ?? null) === null &&
      change.kind !== "run_command",
  );
  const declined = $derived(change?.decision === false);
  /** Once the change settles, the record holds what was actually applied. */
  const applied = $derived.by(() => {
    const record = step?.evidence;
    if (!record) return undefined;
    for (const execution of session?.projection?.executions ?? [])
      for (const entry of execution.file_evidence ?? []) if (entry.id === record) return entry.file;
    return undefined;
  });
  const shown = $derived(applied ?? file);
  const path = $derived(
    shown?.path ??
      (change?.kind === "move_file"
        ? change.from
        : change?.kind === "run_command"
          ? change.cwd
          : (change?.path ?? "")),
  );
  const blocked = $derived(!!session?.pending);
  let deciding = $state(false);
  let loadAttempt = $state(0);
  async function decide(approve: boolean) {
    const current = proposal;
    if (!current || !waiting || blocked || deciding) return;
    deciding = true;
    try {
      if (await current.session.approveStep(current.step, approve)) ondecided?.();
    } finally {
      deciding = false;
    }
  }
</script>

<section class="file">
  <header>
    <span class="where">
      <span class="path">{homePath(path)}</span>
      <span class="facts"
        >{shown
          ? (labels[shown.kind]?.() ?? shown.name)
          : fileName(path)}{#if shown?.truncated}<span class="dot" aria-hidden="true"
          ></span>{m.work_env_file_truncated()}{/if}</span
      >
    </span>
    <span class="actions">
      {#if onreveal && path}<Button size="compact" onclick={() => onreveal?.(path)}
          >{m.work_env_reveal()}</Button
        >{/if}
      {#if onback}<Button size="compact" onclick={onback}>{m.work_env_back()}</Button>{/if}
    </span>
  </header>
  {#if change?.kind === "run_command" && step && session}
    {#key loadAttempt}
      {#await import("./local/CommandInspector.svelte") then module}
        <module.default {step} {session} {ondecided} />
      {:catch}
        <Button onclick={() => loadAttempt++}>{m.work_local_approval_failed()}</Button>
      {/await}
    {/key}
  {:else if shown}
    {#if shown.lines}<p class="facts">
        {m.work_local_lines({
          first: String(shown.lines.first),
          last: String(shown.lines.last),
          total: String(shown.lines.total),
        })}
      </p>{/if}
    {#if shown.text}<pre class="body">{shown.text}</pre>{:else}<p class="empty">
        {m.work_env_file_empty()}
      </p>{/if}
  {:else if declined}
    <p class="empty">{m.work_env_file_declined()}</p>
  {:else if step?.local?.proposal}
    <pre class="body">{step.local.proposal}</pre>
  {:else if change?.kind === "move_file"}
    <p>{m.work_local_move({ from: homePath(change.from), to: homePath(change.to) })}</p>
  {:else if change?.kind === "delete_file"}
    <p>{m.work_local_delete({ path: homePath(change.path) })}</p>
  {:else if change?.kind === "edit_file"}
    <div class="passages">
      <div class="passage removed">
        <span class="label">{m.work_env_file_removed()}</span>
        <pre class="body">{change.old}</pre>
      </div>
      <div class="passage added">
        <span class="label">{m.work_env_file_added()}</span>
        <pre class="body">{change.new}</pre>
      </div>
    </div>
  {:else if change?.kind === "write_file"}
    <div class="passage added">
      <span class="label">{m.work_env_file_proposed()}</span>
      <pre class="body">{change.content}</pre>
    </div>
  {:else}
    <p class="empty">{m.work_env_file_empty()}</p>
  {/if}
  {#if waiting && change?.kind !== "run_command"}
    <footer>
      <Button variant="primary" disabled={blocked || deciding} onclick={() => void decide(true)}
        >{m.work_line_approve_change()}</Button
      >
      <Button disabled={blocked || deciding} onclick={() => void decide(false)}
        >{m.work_line_reject_change()}</Button
      >
    </footer>
  {/if}
</section>

<style>
  .file {
    display: flex;
    flex-direction: column;
    gap: 12px;
    block-size: 100%;
    min-block-size: 0;
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    flex: none;
    padding-inline-end: 28px;
  }

  .where {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }

  .path {
    font-family: var(--font-mono);
    font-size: var(--text-label);
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .facts {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .dot {
    inline-size: 3px;
    block-size: 3px;
    border-radius: 50%;
    background: currentcolor;
  }

  .passages {
    display: flex;
    flex-direction: column;
    gap: 10px;
    flex: 1;
    min-block-size: 0;
    overflow: auto;
  }

  .passage {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
    min-block-size: 0;
  }

  .label {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .body {
    flex: 1;
    min-block-size: 0;
    margin: 0;
    padding: 12px 14px;
    border-radius: var(--radius-control);
    background: var(--color-fill);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    overflow: auto;
  }

  .removed .body {
    background: color-mix(in srgb, var(--color-danger) 12%, var(--color-fill));
  }

  .added .body {
    background: color-mix(in srgb, var(--color-success) 14%, var(--color-fill));
  }

  .empty {
    margin: 0;
    color: var(--color-muted);
  }

  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
</style>

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
    ondecided,
  }: {
    /** What one settled file step disclosed: a listing, an excerpt, hits, or a diff. */
    file?: WorkFileEvidenceV1;
    /** A change a run is proposing, with the session that can answer it. */
    proposal?: { session: WorkSession; step: string };
    onback?: () => void;
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
  const change = $derived(
    step?.kind.kind === "write_file" || step?.kind.kind === "edit_file" ? step.kind : undefined,
  );
  const waiting = $derived(
    !!change && step?.status === "running" && (change.decision ?? null) === null,
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
  const path = $derived(shown?.path ?? change?.path ?? "");
  const blocked = $derived(!!session?.pending);
  let deciding = $state(false);
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
    {#if onback}<Button size="compact" onclick={onback}>{m.work_env_back()}</Button>{/if}
  </header>
  {#if shown}
    {#if shown.text}<pre class="body">{shown.text}</pre>{:else}<p class="empty">
        {m.work_env_file_empty()}
      </p>{/if}
  {:else if declined}
    <p class="empty">{m.work_env_file_declined()}</p>
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
  {#if waiting}
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

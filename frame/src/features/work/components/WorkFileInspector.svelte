<script lang="ts">
  import type { WorkFileEvidenceV1 } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import { homePath } from "../lib/work-files";
  import * as m from "$shared/i18n/messages";
  let {
    file,
    onback,
  }: {
    /** What one settled file step disclosed: a listing, an excerpt, hits, or a diff. */
    file: WorkFileEvidenceV1;
    onback?: () => void;
  } = $props();
  const labels: Record<string, () => string> = {
    directory: m.work_env_file_listing,
    search: m.work_env_file_search,
    written: m.work_env_file_written,
  };
</script>

<section class="file">
  <header>
    <span class="where">
      <span class="path">{homePath(file.path)}</span>
      <span class="facts"
        >{labels[file.kind]?.() ?? file.name}{#if file.truncated}<span
            class="dot"
            aria-hidden="true"
          ></span>{m.work_env_file_truncated()}{/if}</span
      >
    </span>
    {#if onback}<Button size="compact" onclick={onback}>{m.work_env_back()}</Button>{/if}
  </header>
  {#if file.text}<pre class="body">{file.text}</pre>{:else}<p class="empty">
      {m.work_env_file_empty()}
    </p>{/if}
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

  .empty {
    margin: 0;
    color: var(--color-muted);
  }
</style>

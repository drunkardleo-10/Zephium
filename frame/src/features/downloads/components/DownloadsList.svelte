<script lang="ts">
  import { untrack } from "svelte";
  import { DownloadSession, downloadProgress, formatDownloadBytes } from "$domain/downloads";
  import type { DownloadError, DownloadState } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  let { profile }: { profile: string } = $props();
  let session = $state.raw(untrack(() => new DownloadSession(profile)));
  $effect(() => {
    const next = new DownloadSession(profile);
    session = next;
    void next.start();
    return () => next.stop();
  });
  const labels: Record<DownloadState, () => string> = {
    pending: m.download_pending,
    receiving: m.download_receiving,
    cancelling: m.download_cancelling,
    finalizing: m.download_finalizing,
    completed: m.download_completed,
    cancelled: m.download_cancelled,
    interrupted: m.download_interrupted,
    failed: m.download_failed,
  };
  const errors: Record<DownloadError, () => string> = {
    invalid: m.download_error_invalid,
    unavailable: m.download_error_unavailable,
    unsupported: m.download_error_unsupported,
    capacity: m.download_error_capacity,
    storage: m.download_error_storage,
    destination: m.download_error_destination,
    network: m.download_error_network,
    disk_full: m.download_error_disk_full,
    protection: m.download_error_protection,
    missing_file: m.download_error_missing,
    changed_file: m.download_error_changed,
    cancelled: m.download_cancelled,
  };
</script>

<section
  class="downloads-list"
  aria-label={m.browser_downloads_title()}
  aria-busy={session.loading}
>
  {#if session.error}<p role="alert">{errors[session.error]()}</p>
    <Button size="compact" onclick={() => void session.retry()}>{m.surface_retry()}</Button>{/if}
  {#if session.cleanup.error}
    <p role="alert">{m.download_cleanup_error()}</p>
    <Button
      size="compact"
      disabled={session.busy || session.cleanup.running}
      onclick={() => void session.perform({ kind: "retry_cleanup" })}
    >
      {session.cleanup.running ? m.download_cleanup_running() : m.download_cleanup_retry()}
    </Button>
  {/if}
  {#if !session.supported}<p role="status">{m.download_error_unsupported()}</p>{/if}
  {#if !session.entries.length && !session.error}<p class="empty" role="status">
      {session.loading ? m.download_loading() : m.download_empty()}
    </p>{/if}
  <ul>
    {#each session.entries as entry (entry.id)}
      <li>
        <div class="file-copy">
          <strong title={entry.filename}>{entry.filename}</strong><span
            >{entry.source_is_context
              ? m.download_source_context({ origin: entry.source })
              : entry.source}</span
          >
        </div>
        <div class="transfer-status">
          <span>{labels[entry.state]()}</span><span
            >{formatDownloadBytes(entry.received)}{#if entry.total}
              / {formatDownloadBytes(entry.total)}{/if}</span
          >
        </div>
        {#if entry.state === "receiving"}<progress
            aria-label={m.download_progress({ name: entry.filename })}
            max="1"
            value={downloadProgress(entry)}
          ></progress>{/if}
        {#if entry.error}<p class="error">{errors[entry.error]()}</p>{/if}
        <div class="actions">
          {#if entry.state === "pending" || entry.state === "receiving"}<Button
              size="compact"
              disabled={session.busy}
              onclick={() => void session.perform({ kind: "cancel", id: entry.id })}
              >{m.download_cancel()}</Button
            >
          {:else if entry.state === "completed"}<Button
              size="compact"
              disabled={session.busy}
              onclick={() => void session.perform({ kind: "open", id: entry.id })}
              >{m.download_open()}</Button
            ><Button
              size="compact"
              disabled={session.busy}
              onclick={() => void session.perform({ kind: "reveal", id: entry.id })}
              >{m.download_reveal()}</Button
            >{/if}
          {#if ["completed", "failed", "cancelled", "interrupted"].includes(entry.state)}<Button
              variant="ghost"
              size="compact"
              disabled={session.busy}
              onclick={() => void session.perform({ kind: "forget", id: entry.id })}
              >{m.download_forget()}</Button
            >{/if}
        </div>
      </li>
    {/each}
  </ul>
  {#if session.next}<Button
      disabled={session.loading || session.entries.length >= 2000}
      onclick={() => void session.reload(true)}>{m.download_more()}</Button
    >{/if}
</section>

<style>
  .downloads-list {
    display: grid;
    gap: 12px;
    min-width: 0;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: grid;
    gap: 8px;
    padding-block: 16px;
    border-bottom: 1px solid var(--color-border);
  }

  .file-copy {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 500;
    color: var(--color-text);
  }

  .file-copy span,
  .transfer-status,
  .empty,
  .error {
    color: var(--color-label-secondary);
    font-size: 12px;
  }

  .file-copy > span {
    overflow-wrap: anywhere;
  }

  .transfer-status {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  progress {
    width: 100%;
    height: 4px;
    accent-color: var(--color-text);
  }

  p {
    margin: 0;
  }
</style>

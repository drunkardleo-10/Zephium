<script lang="ts">
  import { untrack } from "svelte";
  import { Cancel01Icon, Delete02Icon, FolderOpenIcon } from "@hugeicons/core-free-icons";
  import { DownloadSession, downloadProgress, formatDownloadBytes } from "$domain/downloads";
  import type { DownloadError, DownloadState, DownloadView } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import IconButton from "$shared/ui/IconButton";
  import * as m from "$shared/i18n/messages";
  import FileGlyph from "./FileGlyph.svelte";
  import { TransferRate, transferLine } from "../lib/transfer";
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
  const rates = new TransferRate();
  const day = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });

  function dayOf(entry: DownloadView) {
    const at = new Date(Number(entry.created_at) * 1000);
    const today = new Date();
    const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
    if (at.toDateString() === today.toDateString()) return m.history_today();
    if (at.toDateString() === yesterday.toDateString()) return m.history_yesterday();
    return day.format(at);
  }

  /** Newest first, under the day each began. */
  let days = $derived.by(() => {
    rates.retain(session.entries.map((entry) => entry.id));
    const grouped: { label: string; entries: DownloadView[] }[] = [];
    const newest = session.entries.toSorted((a, b) => Number(b.created_at) - Number(a.created_at));
    for (const entry of newest) {
      const label = dayOf(entry);
      const last = grouped.at(-1);
      if (last?.label === label) last.entries.push(entry);
      else grouped.push({ label, entries: [entry] });
    }
    return grouped;
  });

  function hostOf(source: string) {
    try {
      return new URL(source).host || source;
    } catch {
      return source;
    }
  }

  function line(entry: DownloadView) {
    if (entry.state === "receiving") {
      const rate = rates.observe(entry.id, Number(entry.received));
      return transferLine(entry, rate) || labels.receiving();
    }
    const where = entry.source_is_context
      ? m.download_source_context({ origin: entry.source })
      : hostOf(entry.source);
    if (entry.state === "completed")
      return `${formatDownloadBytes(entry.total ?? entry.received)} · ${where}`;
    return `${labels[entry.state]()} · ${where}`;
  }
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
  {#each days as group (group.label)}
    <h3 class="day">{group.label}</h3>
    <ul>
      {#each group.entries as entry (entry.id)}
        {@const progress = downloadProgress(entry)}
        <li data-state={entry.state}>
          <FileGlyph filename={entry.filename} />
          <div class="copy">
            {#if entry.state === "completed"}<button
                type="button"
                class="name"
                title={entry.filename}
                aria-label={m.download_open_named({ name: entry.filename })}
                disabled={session.busy}
                onclick={() => void session.perform({ kind: "open", id: entry.id })}
                >{entry.filename}</button
              >{:else}<span class="name" title={entry.filename}>{entry.filename}</span>{/if}
            <span class="line">{line(entry)}</span>
            {#if entry.state === "receiving"}<progress
                aria-label={m.download_progress({ name: entry.filename })}
                max="1"
                value={progress}
              ></progress>{/if}
            {#if entry.error}<span class="error">{errors[entry.error]()}</span>{/if}
          </div>
          <div class="actions">
            {#if entry.state === "pending" || entry.state === "receiving"}<IconButton
                icon={Cancel01Icon}
                label={m.download_cancel()}
                size={13}
                buttonSize={26}
                disabled={session.busy}
                onclick={() => void session.perform({ kind: "cancel", id: entry.id })}
              />{/if}
            {#if entry.state === "completed"}<IconButton
                icon={FolderOpenIcon}
                label={m.download_reveal()}
                size={14}
                buttonSize={26}
                disabled={session.busy}
                onclick={() => void session.perform({ kind: "reveal", id: entry.id })}
              />{/if}
            {#if ["completed", "failed", "cancelled", "interrupted"].includes(entry.state)}<IconButton
                icon={Delete02Icon}
                label={m.download_forget()}
                size={14}
                buttonSize={26}
                disabled={session.busy}
                onclick={() => void session.perform({ kind: "forget", id: entry.id })}
              />{/if}
          </div>
        </li>
      {/each}
    </ul>
  {/each}
  {#if session.next}<Button
      disabled={session.loading || session.entries.length >= 2000}
      onclick={() => void session.reload(true)}>{m.download_more()}</Button
    >{/if}
</section>

<style>
  .downloads-list {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .day {
    margin: 10px 4px 2px;
    color: var(--color-faint);
    font-size: 11px;
    font-weight: 550;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px;
    border-radius: var(--radius-row);
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  li:hover,
  li:focus-within {
    background: var(--row-active);
  }

  .copy {
    display: grid;
    flex: 1;
    gap: 3px;
    min-width: 0;
  }

  .name {
    overflow: hidden;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    text-align: start;
    white-space: nowrap;
    text-overflow: ellipsis;
    cursor: default;
  }

  button.name:hover:not(:disabled) {
    text-decoration: underline;
  }

  .line,
  .empty,
  .error {
    color: var(--color-muted);
    font-size: 11.5px;
  }

  .line {
    overflow: hidden;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .error,
  li:is([data-state="failed"], [data-state="interrupted"]) .line {
    color: var(--color-danger);
  }

  .empty {
    padding: 8px 4px;
  }

  /* Faded rather than hidden, so the keyboard can still reach them. */
  .actions {
    display: flex;
    gap: 2px;
    opacity: 0;
    transition: opacity var(--motion-instant) var(--ease-smooth);
  }

  li:hover .actions,
  li:focus-within .actions,
  li[data-state="receiving"] .actions,
  li[data-state="pending"] .actions {
    opacity: 1;
  }

  progress {
    width: 100%;
    height: 3px;
    overflow: hidden;
    border: 0;
    border-radius: 2px;
    appearance: none;
    background: var(--color-fill);
  }

  progress::-webkit-progress-bar {
    background: var(--color-fill);
  }

  progress::-webkit-progress-value {
    border-radius: 2px;
    background: var(--color-accent);
    transition: inline-size var(--motion-base) var(--ease-smooth);
  }

  p {
    margin: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    li,
    .actions,
    progress::-webkit-progress-value {
      transition: none;
    }
  }
</style>

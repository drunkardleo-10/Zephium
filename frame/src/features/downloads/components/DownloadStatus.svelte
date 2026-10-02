<script lang="ts">
  import { untrack } from "svelte";
  import { Cancel01Icon, FolderOpenIcon } from "@hugeicons/core-free-icons";
  import { DownloadSession, downloadProgress, formatDownloadBytes } from "$domain/downloads";
  import type { DownloadView } from "$shared/ipc/bindings";
  import IconButton from "$shared/ui/IconButton";
  import * as m from "$shared/i18n/messages";
  import FileGlyph from "./FileGlyph.svelte";
  import { TransferRate, transferLine } from "../lib/transfer";

  let { profile, onopen }: { profile: string; onopen: () => void } = $props();
  let session = $state.raw(untrack(() => new DownloadSession(profile)));
  let showTerminal = $state(false);
  let dismissed = $state<string | null>(null);
  const rates = new TransferRate();
  $effect(() => {
    const next = new DownloadSession(profile);
    session = next;
    // Read one bounded native snapshot, then listen without idle polling.
    void next.start(false);
    return () => next.stop();
  });

  const live = (entry: DownloadView) =>
    ["pending", "receiving", "cancelling", "finalizing"].includes(entry.state);
  let active = $derived(session.entries.filter(live));
  let current = $derived(active[0] ?? session.entries[0]);
  let progress = $derived(current ? downloadProgress(current) : undefined);
  let rate = $derived.by(() => {
    rates.retain(active.map((entry) => entry.id));
    return current?.state === "receiving"
      ? rates.observe(current.id, Number(current.received))
      : null;
  });
  let line = $derived.by(() => {
    if (!current) return "";
    switch (current.state) {
      case "receiving":
        return transferLine(current, rate) || m.download_receiving();
      case "pending":
        return m.download_pending();
      case "cancelling":
        return m.download_cancelling();
      case "finalizing":
        return m.download_finalizing();
      case "completed":
        return `${m.download_completed()} · ${formatDownloadBytes(current.total ?? current.received)}`;
      case "cancelled":
        return m.download_cancelled();
      case "interrupted":
        return m.download_interrupted();
      default:
        return m.download_failed();
    }
  });
  let terminalKey = $derived(
    active.length === 0 && current ? `${current.id}:${current.state}` : "",
  );
  $effect(() => {
    showTerminal = Boolean(terminalKey) && terminalKey !== dismissed;
    if (!showTerminal) return;
    const timer = setTimeout(() => {
      showTerminal = false;
    }, 10_000);
    return () => clearTimeout(timer);
  });
</script>

{#if current && (active.length > 0 || showTerminal)}
  <div class="download-card" data-state={current.state}>
    <button class="body" type="button" onclick={onopen} aria-label={m.download_show_status()}>
      <FileGlyph filename={current.filename} size={30} />
      <span class="copy">
        <strong title={current.filename}>{current.filename}</strong>
        <span class="line"
          >{line}{#if active.length > 1}<span class="more"
              >{m.download_more_active({ count: active.length - 1 })}</span
            >{/if}</span
        >
      </span>
    </button>
    {#if current.state === "pending" || current.state === "receiving"}
      <IconButton
        icon={Cancel01Icon}
        label={m.download_cancel()}
        size={13}
        buttonSize={24}
        disabled={session.busy}
        onclick={() => void session.perform({ kind: "cancel", id: current.id })}
      />
    {:else if current.state === "completed"}
      <IconButton
        icon={FolderOpenIcon}
        label={m.download_reveal()}
        size={14}
        buttonSize={24}
        disabled={session.busy}
        onclick={() => void session.perform({ kind: "reveal", id: current.id })}
      />
    {:else if active.length === 0}
      <IconButton
        icon={Cancel01Icon}
        label={m.download_dismiss()}
        size={13}
        buttonSize={24}
        onclick={() => (dismissed = terminalKey)}
      />
    {/if}
    {#if active.length > 0}
      <div
        class="track"
        role="progressbar"
        aria-label={m.download_progress({ name: current.filename })}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={progress === undefined ? undefined : Math.round(progress * 100)}
        data-indeterminate={progress === undefined}
      >
        <i style:transform={progress === undefined ? undefined : `scaleX(${progress})`}></i>
      </div>
    {/if}
  </div>
{/if}

<style>
  .download-card {
    position: relative;
    display: flex;
    flex: none;
    align-items: center;
    gap: 2px;
    margin: 6px 8px;
    padding: 8px 6px 10px 8px;
    overflow: hidden;
    border-radius: var(--radius-row);
    background: var(--row-active);
    box-shadow: var(--row-rim);
    animation: card-in var(--motion-base) var(--ease-emphasized) both;
  }

  .body {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .copy {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  strong {
    overflow: hidden;
    color: var(--color-text);
    font-size: var(--text-label);
    font-weight: 550;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .line {
    overflow: hidden;
    color: var(--color-muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .more {
    margin-inline-start: 6px;
    color: var(--color-faint);
  }

  .download-card[data-state="completed"] :global(.glyph) {
    color: var(--color-success);
  }

  .download-card:is([data-state="failed"], [data-state="interrupted"]) .line {
    color: var(--color-danger);
  }

  .track {
    position: absolute;
    inset-inline: 10px;
    inset-block-end: 4px;
    height: 3px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--color-fill);
  }

  .track i {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--color-accent);
    transform-origin: left center;
    transition: transform var(--motion-base) var(--ease-smooth);
  }

  .track:dir(rtl) i {
    transform-origin: right center;
  }

  .track[data-indeterminate="true"] i {
    width: 35%;
    animation: sweep 1.2s var(--ease-smooth) infinite;
  }

  .body:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
    border-radius: var(--radius-inset);
  }

  @keyframes card-in {
    from {
      opacity: 0;
      translate: 0 6px;
    }
  }

  @keyframes sweep {
    from {
      translate: -100% 0;
    }

    to {
      translate: 300% 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .track i {
      transition: none;
    }

    .download-card,
    .track[data-indeterminate="true"] i {
      animation: none;
    }
  }
</style>

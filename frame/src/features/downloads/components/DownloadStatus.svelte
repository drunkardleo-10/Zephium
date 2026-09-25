<script lang="ts">
  import { untrack } from "svelte";
  import { DownloadSession, downloadProgress } from "$domain/downloads";
  import Icon from "$shared/ui/Icon";
  import { Download01Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  let { profile, onopen }: { profile: string; onopen: () => void } = $props();
  let session = $state.raw(untrack(() => new DownloadSession(profile)));
  let showTerminal = $state(false);
  $effect(() => {
    const next = new DownloadSession(profile);
    session = next;
    // Read one bounded native snapshot, then listen without idle polling.
    void next.start(false);
    return () => next.stop();
  });
  let active = $derived(
    session.entries.find((entry) =>
      ["pending", "receiving", "cancelling", "finalizing"].includes(entry.state),
    ),
  );
  let current = $derived(active ?? session.entries[0]);
  let label = $derived(
    current?.state === "pending"
      ? m.download_pending()
      : current?.state === "cancelling"
        ? m.download_cancelling()
        : current?.state === "finalizing"
          ? m.download_finalizing()
          : current?.state === "receiving"
            ? m.download_receiving()
            : current?.state === "completed"
              ? m.download_completed()
              : current?.state === "cancelled"
                ? m.download_cancelled()
                : m.download_failed(),
  );
  let terminalKey = $derived(!active && current ? `${current.id}:${current.state}` : "");
  $effect(() => {
    showTerminal = Boolean(terminalKey);
    if (!terminalKey) return;
    const timer = setTimeout(() => {
      showTerminal = false;
    }, 10_000);
    return () => clearTimeout(timer);
  });
</script>

{#if current && (active || showTerminal)}
  <button class="download-status" onclick={onopen} aria-label={m.download_show_status()}>
    <Icon icon={Download01Icon} size={16} />
    <span class="copy"><strong>{current.filename}</strong><span>{label}</span></span>
    {#if current.state === "receiving" && downloadProgress(current) !== undefined}<span
        >{Math.round(downloadProgress(current)! * 100)}%</span
      >{/if}
  </button>
{/if}

<style>
  .download-status {
    display: flex;
    align-items: center;
    gap: 8px;
    width: calc(100% - 16px);
    margin: 8px;
    padding: 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-row);
    color: var(--color-text);
    background: var(--color-fill);
    font-size: 11px;
    text-align: start;
  }

  .copy {
    display: grid;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  strong {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .copy > span {
    color: var(--color-label-secondary);
  }

  button:focus-visible {
    outline: 2px solid var(--color-text);
    outline-offset: 2px;
  }
</style>

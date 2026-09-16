<script lang="ts">
  import { untrack } from "svelte";
  import { ArrowLeft02Icon, Clock01Icon } from "@hugeicons/core-free-icons";
  import type { HistoryRange } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import EmptyState from "$shared/ui/EmptyState";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import Button from "$shared/ui/Button";
  import SearchField from "$shared/ui/SearchField";
  import Select from "$shared/ui/Select";
  import { surface as browser } from "$domain/surface";
  import { tabs } from "$domain/tabs";
  import * as m from "$shared/i18n/messages";
  import { historySession } from "../lib/history-session.svelte";
  import HistoryList from "./HistoryList.svelte";

  let profile = $derived(tabs.profile()?.id ?? "unbound");
  let session = $state.raw(untrack(() => historySession(profile, "page")));
  let query = $state("");

  // Only the owning profile decides which session this is. Starting one reads
  // its own state, so a tracked body would restart the session on every change
  // it made — including the query the reader just typed.
  $effect(() => {
    const owner = profile;
    return untrack(() => {
      const current = historySession(owner, "page");
      session = current;
      void current.start();
      return () => current.stop();
    });
  });

  let range = $state<HistoryRange>("hour");
  const ranges = [
    { value: "hour", label: m.history_clear_hour() },
    { value: "day", label: m.history_clear_day() },
    { value: "week", label: m.history_clear_week() },
    { value: "everything", label: m.history_clear_all() },
  ];

  // Navigating in place returns from this page natively, so the row lands in
  // the tab the reader was already looking at.
  const open = (url: string, newTab: boolean) => commands.browserOpenUrl(url, newTab);
</script>

<section class="library-shell">
  <header class="internal-toolbar">
    <IconButton
      icon={ArrowLeft02Icon}
      label={m.settings_back()}
      onclick={() => void browser.open(null)}
    /><span class="toolbar-current">{m.browser_history_title()}</span>
    <div class="controls">
      <SearchField
        label={m.history_search()}
        placeholder={m.history_search()}
        size="chrome"
        value={query}
        oninput={(value) => {
          query = value;
          session.search(value);
        }}
      />
      <Select
        label={m.preview_time_range()}
        labelHidden
        options={ranges}
        value={range}
        onchange={(value) => (range = value as HistoryRange)}
      />
      <Button
        variant="danger"
        size="compact"
        disabled={session.busy}
        onclick={() => void session.clear(range)}>{m.history_clear()}</Button
      >
    </div>
  </header>

  {#if session.error}
    <EmptyState title={m.history_unavailable()} description={m.surface_retry()}>
      {#snippet icon()}<Icon icon={Clock01Icon} size={28} />{/snippet}
      {#snippet action()}
        <button type="button" class="retry" onclick={() => session.retry()}
          >{m.surface_retry()}</button
        >
      {/snippet}
    </EmptyState>
  {:else if session.empty}
    <EmptyState
      title={query.trim() ? m.history_no_matches() : m.browser_history_empty()}
      description={query.trim() ? m.history_no_matches_help() : m.history_empty_help()}
    >
      {#snippet icon()}<Icon icon={Clock01Icon} size={28} />{/snippet}
    </EmptyState>
  {:else}
    <div class="list">
      <HistoryList {session} onopen={(url, newTab) => void open(url, newTab)} />
    </div>
  {/if}
</section>

<style>
  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    margin-inline-start: auto;
  }

  .controls :global(.ui-search) {
    min-width: 0;
    width: clamp(120px, 26vw, 260px);
  }

  .list {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    width: 100%;
    max-width: 920px;
    margin-inline: auto;
    padding-inline: clamp(12px, 4vw, 48px);
    padding-block-end: 24px;
  }

  .retry {
    height: var(--control-regular);
    padding-inline: 14px;
    border: 0;
    border-radius: var(--radius-control);
    background: var(--color-control);
    font-size: 12px;
    color: var(--color-on-control);
    cursor: default;
  }
</style>

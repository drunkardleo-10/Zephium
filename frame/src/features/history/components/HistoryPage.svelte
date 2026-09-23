<script lang="ts">
  import { untrack } from "svelte";
  import { ArrowLeft02Icon, Clock01Icon } from "@hugeicons/core-free-icons";
  import type { HistoryRange } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import EmptyState from "$shared/ui/EmptyState";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
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
  let armed = $state(false);

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

  const ranges = [
    { value: "everything", label: m.history_range_all() },
    { value: "hour", label: m.history_range_hour() },
    { value: "day", label: m.history_range_day() },
    { value: "week", label: m.history_range_week() },
  ];
  let confirmLabel = $derived(
    {
      everything: m.history_clear_confirm_all(),
      hour: m.history_clear_confirm_hour(),
      day: m.history_clear_confirm_day(),
      week: m.history_clear_confirm_week(),
    }[session.range],
  );

  // Navigating in place returns from this page natively, so the row lands in
  // the tab the reader was already looking at.
  const open = (url: string, newTab: boolean) => commands.browserOpenUrl(url, newTab);

  // Clearing cannot be undone, so the button states what it is about to remove
  // and only acts on a second, deliberate press.
  function clear() {
    if (!armed) {
      armed = true;
      return;
    }
    armed = false;
    void session.clear();
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape") armed = false;
  }}
/>

<section class="library-shell">
  <header class="internal-toolbar">
    <IconButton
      icon={ArrowLeft02Icon}
      label={m.settings_back()}
      onclick={() => void browser.open(null)}
    /><span class="toolbar-current">{m.browser_history_title()}</span>
  </header>

  <div class="content">
    <div class="controls">
      <div class="bar">
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
          label={m.history_range()}
          labelHidden
          options={ranges}
          value={session.range}
          onchange={(value) => {
            armed = false;
            session.scope(value as HistoryRange);
          }}
        />
        <Button
          variant={armed ? "danger" : "secondary"}
          disabled={session.busy || session.empty}
          onblur={() => (armed = false)}
          onclick={clear}>{armed ? confirmLabel : m.history_clear()}</Button
        >
      </div>
    </div>

    {#if session.error}
      <div class="empty">
        <EmptyState title={m.history_unavailable()} description={m.history_unavailable_help()}>
          {#snippet icon()}<Icon icon={Clock01Icon} size={28} />{/snippet}
          {#snippet action()}
            <Button onclick={() => session.retry()}>{m.surface_retry()}</Button>
          {/snippet}
        </EmptyState>
      </div>
    {:else if session.empty}
      <div class="empty">
        <EmptyState
          title={query.trim() ? m.history_no_matches() : m.browser_history_empty()}
          description={query.trim() ? m.history_no_matches_help() : m.history_empty_help()}
        >
          {#snippet icon()}<Icon icon={Clock01Icon} size={28} />{/snippet}
        </EmptyState>
      </div>
    {:else}
      <HistoryList {session} onopen={(url, newTab) => void open(url, newTab)} />
    {/if}
  </div>
</section>

<style>
  /* The reading column, published for the list as well: the controls and the
     rows they act on have to sit on the same two edges, and the list makes its
     column out of the scroller's padding rather than a wrapper. */
  .content {
    --library-gutter: clamp(16px, 4vw, 48px);
    --library-column: 820px;

    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .controls {
    flex: none;
    padding-inline: max(var(--library-gutter), calc((100% - var(--library-column)) / 2));

    /* The scroller below reserves a 12px gutter for its bar, so the bar over
       it has to give up the same 12px or the two columns sit 6px apart. */
    padding-inline-end: max(var(--library-gutter), calc((100% - var(--library-column)) / 2 + 12px));
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding-block: 4px 14px;
  }

  .bar :global(.ui-search) {
    flex: 1;
    min-width: 0;
  }

  .empty {
    padding-inline: var(--library-gutter);
  }
</style>

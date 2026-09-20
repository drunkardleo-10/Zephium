<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { rememberScroll } from "$shared/ui/scroll-memory";
  import { Add01Icon } from "@hugeicons/core-free-icons";
  import { tabs } from "$domain/tabs";
  import Icon from "$shared/ui/Icon";
  import type { SidebarEntry } from "../lib/sidebar-model";
  import TabList from "./TabList.svelte";

  let {
    pinned,
    today,
    splitting,
    onSelect,
  }: {
    pinned: SidebarEntry[];
    today: SidebarEntry[];
    splitting: boolean;
    onSelect: (id: string) => void;
  } = $props();
</script>

<!--
  The only scrolling region in the chrome. Everything above it is pinned so a
  long tab list can never push a scrollbar up alongside the navigation row.

  Pinned tabs are separated by a rule rather than a heading: the rule says
  "these stay" without spending a row on a word, and the rows themselves carry
  the difference by having no close control.
-->
<div
  data-tabs-drop
  use:rememberScroll={`${tabs.profile()?.id}/${tabs.activeSpaceId()}/tabs`}
  class="tab-scroller"
>
  <TabList entries={pinned} section="pinned" label={m.pinned_tabs()} {splitting} {onSelect} />

  {#if pinned.length > 0}
    <div class="tab-divider" aria-hidden="true"></div>
  {/if}

  <div class="new-tab-slot">
    <button type="button" class="new-tab" onclick={tabs.open}>
      <Icon icon={Add01Icon} size={16} />
      <span>{m.new_tab()}</span>
    </button>
  </div>

  <TabList entries={today} section="today" label={m.open_tabs()} {splitting} {onSelect} />
</div>

<style>
  .tab-scroller {
    min-height: 0;
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding-block-end: 4px;
  }

  .tab-divider {
    height: 1px;
    margin: 5px calc(var(--sidebar-inset) + 8px);
    background: var(--color-border);
  }

  .new-tab-slot {
    padding-inline: var(--sidebar-inset);
    padding-block: 2px calc(var(--sidebar-row-gap) + 2px);
  }

  .new-tab {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: var(--row-sidebar);
    padding-inline: 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-faint);
    font: inherit;
    font-size: var(--sidebar-row-text);
    font-weight: var(--sidebar-row-weight);
    letter-spacing: -0.005em;
    text-align: start;
    cursor: default;
    outline: none;
    transition:
      background-color var(--motion-fast) var(--ease-out-quiet),
      color var(--motion-fast) var(--ease-out-quiet);
  }

  .new-tab:hover {
    background: var(--row-hover);
    color: var(--color-label-secondary);
  }
</style>

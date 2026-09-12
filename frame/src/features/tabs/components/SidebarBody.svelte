<script lang="ts">
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
-->
<div
  data-tabs-drop
  use:rememberScroll={`${tabs.profile()?.id}/${tabs.activeSpaceId()}/tabs`}
  class="min-h-0 flex-1 overflow-y-auto overscroll-contain pb-1"
>
  <TabList entries={pinned} section="pinned" label="Pinned tabs" {splitting} {onSelect} />

  {#if pinned.length > 0}
    <div class="mx-3 my-1 border-t border-border" aria-hidden="true"></div>
  {/if}

  <div class="px-1.5 pt-0.5">
    <button
      type="button"
      class="flex h-[34px] w-full items-center gap-2.5 rounded-md px-2 text-start text-[13.5px] text-text transition-[background-color,color] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none hover:bg-fill-hover hover:text-text"
      onclick={tabs.open}
    >
      <Icon icon={Add01Icon} size={16} class="shrink-0" />
      <span>New tab</span>
    </button>
  </div>

  <TabList entries={today} section="today" label="Open tabs" {splitting} {onSelect} />
</div>

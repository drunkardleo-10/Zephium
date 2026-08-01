<script lang="ts">
  import { untrack } from "svelte";
  import * as tabs from "../../domain/tabs/tabs.svelte";
  import * as ui from "../../domain/ui-commands/ui-commands.svelte";
  import AddressField from "./address/AddressField.svelte";
  import EssentialsEmpty from "./essentials/EssentialsEmpty.svelte";
  import EssentialsRail from "./essentials/EssentialsRail.svelte";
  import SidebarFooter from "./footer/SidebarFooter.svelte";
  import SidebarHeader from "./header/SidebarHeader.svelte";
  import { effectiveWidth, isCompact, toggleMode } from "./sidebar-mode.svelte";
  import SidebarResizeHandle from "./SidebarResizeHandle.svelte";
  import SpaceHeader from "./space/SpaceHeader.svelte";
  import { sidebarTree } from "./tabs/sidebar-model";
  import SidebarBody from "./tabs/SidebarBody.svelte";
  import TabList from "./tabs/TabList.svelte";
  import TabRail from "./tabs/TabRail.svelte";

  let splitting = $state(false);
  let tree = $derived(sidebarTree(tabs.sidebarNodes(), tabs.tabs()));
  let compact = $derived(isCompact());
  let width = $derived(effectiveWidth());

  // The rail shows one flat list. Folders and split grouping are shapes that
  // need labels, so they stay in the expanded body.
  let railTabs = $derived(tree.today.flatMap((entry) => (entry.kind === "tab" ? [entry.tab] : [])));
  let railEssentials = $derived(
    tree.favorites.flatMap((entry) => (entry.kind === "tab" ? [entry.tab] : [])),
  );

  // Dispatch runs untracked and behind a sequence guard. Handlers read the
  // state they mutate (the sidebar shape, the tab projection), so a tracked
  // effect would subscribe to its own writes and re-fire on the same command,
  // which made the collapsed menu's Compact Mode toggle immediately undo
  // itself.
  let handledCommand = 0;
  $effect(() => {
    const command = ui.uiCommand();
    if (command.seq === 0 || command.seq === handledCommand) return;
    handledCommand = command.seq;

    untrack(() => {
      if (command.id === "split.choose") splitting = true;
      if (command.id === "tab.copyLink") tabs.copyMenuTargetLink();
      if (command.id === "sidebar.toggleCompact") toggleMode();
    });
  });

  function selectTab(id: string) {
    if (splitting) {
      tabs.split(id);
      splitting = false;
      return;
    }
    tabs.activate(id);
  }
</script>

<aside
  data-tauri-drag-region="deep"
  aria-label="Browser sidebar"
  style:width={`${width}px`}
  style:--sidebar-width={`${width}px`}
  class="relative flex shrink-0 flex-col text-text select-none"
>
  <SidebarResizeHandle {width} />

  <SidebarHeader {compact} />
  <AddressField {compact} />

  {#if compact}
    <EssentialsRail entries={railEssentials} onSelect={selectTab} />
    <TabRail entries={railTabs} onSelect={selectTab} />
  {:else}
    <div class="shrink-0 pb-1.5">
      {#if tree.favorites.length > 0}
        <TabList
          entries={tree.favorites}
          section="favorites"
          variant="essentials"
          label="Essentials"
          {splitting}
          onSelect={selectTab}
        />
      {:else}
        <div class="px-1.5">
          <EssentialsEmpty />
        </div>
      {/if}
    </div>

    <SpaceHeader />

    {#if splitting}
      <p class="shrink-0 px-3 pb-1 text-[12px] text-accent" aria-live="polite">
        Choose a second tab for split view
      </p>
    {/if}

    <SidebarBody pinned={tree.pinned} today={tree.today} {splitting} onSelect={selectTab} />
  {/if}

  <SidebarFooter {compact} />
</aside>

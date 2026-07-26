<script lang="ts">
  import * as tabs from "../../state/tabs.svelte";
  import * as ui from "../../state/ui.svelte";
  import AddressField from "./AddressField.svelte";
  import { sidebarTree } from "./sidebar-model";
  import SidebarBody from "./SidebarBody.svelte";
  import SidebarFooter from "./SidebarFooter.svelte";
  import SidebarHeader from "./SidebarHeader.svelte";
  import SidebarResizeHandle from "./SidebarResizeHandle.svelte";
  import SpaceHeader from "./SpaceHeader.svelte";
  import TabList from "./TabList.svelte";

  const isMac = navigator.userAgent.includes("Mac");

  let width = $state(240);
  let splitting = $state(false);
  let tree = $derived(sidebarTree(tabs.sidebarNodes(), tabs.tabs()));

  $effect(() => {
    const command = ui.uiCommand();
    if (command.seq === 0) return;
    if (command.id === "split.choose") splitting = true;
    if (command.id === "tab.copyLink") tabs.copyMenuTargetLink();
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
  <SidebarResizeHandle {width} onWidthChange={(next) => (width = next)} />

  <SidebarHeader {isMac} />
  <AddressField />

  {#if tree.favorites.length > 0}
    <div class="shrink-0 pb-1.5">
      <TabList
        entries={tree.favorites}
        section="favorites"
        variant="essentials"
        label="Essentials"
        {splitting}
        onSelect={selectTab}
      />
    </div>
  {/if}

  <SpaceHeader />

  {#if splitting}
    <p class="shrink-0 px-3 pb-1 text-[12px] text-accent" aria-live="polite">
      Choose a second tab for split view
    </p>
  {/if}

  <SidebarBody pinned={tree.pinned} today={tree.today} {splitting} onSelect={selectTab} />
  <SidebarFooter />
</aside>

<script lang="ts">
  import { IS_MAC } from "../../shared/platform";
  import * as tabs from "../../domain/tabs/tabs.svelte";
  import * as ui from "../../domain/ui-commands/ui-commands.svelte";
  import AddressField from "./address/AddressField.svelte";
  import EssentialsEmpty from "./essentials/EssentialsEmpty.svelte";
  import { sidebarTree } from "./tabs/sidebar-model";
  import SidebarBody from "./tabs/SidebarBody.svelte";
  import SidebarFooter from "./footer/SidebarFooter.svelte";
  import SidebarHeader from "./header/SidebarHeader.svelte";
  import SidebarResizeHandle from "./SidebarResizeHandle.svelte";
  import SpaceHeader from "./space/SpaceHeader.svelte";
  import TabList from "./tabs/TabList.svelte";

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

  <SidebarHeader isMac={IS_MAC} />
  <AddressField />

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
  <SidebarFooter />
</aside>

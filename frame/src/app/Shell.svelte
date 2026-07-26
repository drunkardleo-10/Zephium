<script lang="ts">
  import NewTab from "../features/newtab/NewTab.svelte";
  import Sidebar from "../features/sidebar/Sidebar.svelte";
  import * as tabs from "../state/tabs.svelte";

  const isMac = navigator.userAgent.includes("Mac");
</script>

<div
  class="shell flex h-screen w-screen"
  class:p-2={!isMac}
  data-zephium-active-tab={tabs.activeId() ?? ""}
>
  <Sidebar />
  {#if !tabs.activeTab()?.url}
    <!--
      Occupies exactly the rect a content WebView would, so moving between a
      page and the new tab never changes the window's shape. The inline start
      inset matches the core layout gap between chrome and content.
    -->
    <main class="min-w-0 flex-1 ps-2" data-zephium-new-tab>
      <div class="content-pane h-full w-full overflow-hidden">
        <NewTab />
      </div>
    </main>
  {/if}
</div>

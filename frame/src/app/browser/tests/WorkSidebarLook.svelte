<script lang="ts">
  import "$styles/global.css";
  import { Dock } from "$features/dock";
  import { ModeTabs } from "$features/sidebar";
  import { loadWorkSidebar } from "$features/work";
  import LazyView from "$shared/ui/LazyView";

  let { compact = false }: { compact?: boolean } = $props();
</script>

<div class={["browser-sidebar", "column", compact && "column-rail"]}>
  <div class="sidebar-columns" data-glide-host>
    <div class="sidebar-browser-column">
      {#if compact}
        <ModeTabs compact standalone />
      {:else}
        <div class="sidebar-head"><ModeTabs /></div>
      {/if}
      <div class="sidebar-mode-body">
        <LazyView loader={loadWorkSidebar} loadingLabel="" failureLabel="" retryLabel=""
          >{#snippet children(Projects)}<Projects
              profile="profile"
              space="space"
              {compact}
            />{/snippet}</LazyView
        >
      </div>
      {#if compact}<Dock compact />{:else}<Dock />{/if}
    </div>
  </div>
</div>

<style>
  .column {
    display: flex;
    flex-direction: column;
    width: 240px;
    height: 640px;
    padding-block-start: 12px;
    background: var(--color-chrome);
  }

  .column-rail {
    align-items: center;
    width: 56px;
  }

  .column :global(.sidebar-browser-column) {
    align-items: stretch;
  }

  .column-rail :global(.sidebar-browser-column) {
    align-items: center;
  }

  .sidebar-mode-body {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }
</style>

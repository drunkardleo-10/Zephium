<script lang="ts">
  import "$styles/global.css";
  import { DownloadStatus } from "$features/downloads";
  import { Dock } from "$features/dock";
  import { UpdateCards, UpdateGlyph } from "$features/updates";

  let { compact = false, download = false }: { compact?: boolean; download?: boolean } = $props();
  const noop = () => {};
</script>

<div class={["browser-sidebar", "column", compact && "column-rail"]}>
  <div class="sidebar-columns">
    <div class="sidebar-browser-column">
      <div class="rest"></div>
      {#if compact}
        <UpdateGlyph onabout={noop} />
        <Dock compact />
      {:else}
        {#if download}<DownloadStatus profile="look" onopen={noop} />{/if}
        <UpdateCards />
        <Dock />
      {/if}
    </div>
  </div>
</div>

<style>
  .column {
    display: flex;
    flex-direction: column;
    width: 240px;
    height: 420px;
    background: var(--color-chrome);
  }

  .column-rail {
    align-items: center;
    width: 56px;
  }

  .rest {
    flex: 1;
  }

  .column-rail :global(.sidebar-browser-column) {
    align-items: center;
  }
</style>

<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { FolderAddIcon } from "../../lib/icons";
  import { homePath } from "../../lib/work-files";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const path = $derived(homePath(item.detail));
</script>

<CardFrame kind={item.kind} title={item.title} icon={FolderAddIcon} {selected} dense>
  {#snippet footer()}<span class="path" title={path}>{path}</span><span class="granted"
      >{m.work_env_folder_granted()}</span
    >{/snippet}
</CardFrame>

<style>
  .path {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: start;
  }

  .granted {
    flex: none;
    color: var(--color-muted);
  }
</style>

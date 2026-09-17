<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { FolderAddIcon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
</script>

<CardFrame kind={item.kind} title={item.title} icon={FolderAddIcon} {selected} dense>
  {#snippet footer()}<span class="path">{item.detail}</span>{/snippet}
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
</style>

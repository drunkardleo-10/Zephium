<script lang="ts">
  import FavIcon from "$shared/ui/FavIcon";
  import CardFrame from "./CardFrame.svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
</script>

<CardFrame
  kind={item.detail || item.kind}
  title={item.title}
  {selected}
  unavailable={item.unavailable}
  dense
>
  {#snippet leading()}<FavIcon favicon={item.favicon ?? null} />{/snippet}
  {#snippet footer()}<span>{item.status}</span>{/snippet}
</CardFrame>

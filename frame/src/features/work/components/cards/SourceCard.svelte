<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { Link04Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
</script>

<CardFrame
  kind={item.detail || item.kind}
  title={item.title}
  icon={Link04Icon}
  {selected}
  unavailable={item.unavailable}
  dense
>
  {#snippet footer()}<span class="role">{item.source?.role ?? item.status}</span>{/snippet}
</CardFrame>

<style>
  .role {
    text-transform: capitalize;
  }
</style>

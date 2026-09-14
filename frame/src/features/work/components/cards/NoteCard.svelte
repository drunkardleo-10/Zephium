<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { Note01Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const updated = $derived.by(() => {
    const date = new Date(item.detail);
    return Number.isNaN(date.getTime())
      ? ""
      : date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  });
</script>

<CardFrame
  kind={item.kind}
  title={item.title}
  icon={Note01Icon}
  {selected}
  unavailable={item.unavailable}
>
  {#snippet footer()}<span>{item.status}</span><span>{updated}</span>{/snippet}
</CardFrame>

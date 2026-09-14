<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { Target01Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected, onaction }: { item: CanvasItem; selected: boolean; onaction: () => void } =
    $props();
  const lines = $derived(item.detail.split("\n").filter(Boolean));
</script>

<CardFrame kind={item.kind} title={item.title} icon={Target01Icon} {selected}>
  <p class="summary">{lines[0] ?? ""}</p>
  {#snippet footer()}<span>{lines[1] ?? item.status}</span>{#if item.actionLabel}<button
        type="button"
        class="link nodrag nopan"
        onclick={(event) => {
          event.stopPropagation();
          onaction();
        }}>{item.actionLabel}</button
      >{/if}{/snippet}
</CardFrame>

<style>
  .summary {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
    overflow: hidden;
  }

  .link {
    border: 0;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
  }

  .link:hover {
    background: var(--color-fill-hover);
  }
</style>

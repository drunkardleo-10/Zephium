<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { Task01Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const outputs = $derived(item.responsibility?.outputs ?? []);
</script>

<CardFrame kind={item.kind} title={item.title} icon={Task01Icon} {selected}>
  {#if outputs.length}<ul class="outputs">
      {#each outputs.slice(0, 3) as output, index (index)}<li>{output}</li>{/each}
      {#if outputs.length > 3}<li class="more">+{outputs.length - 3}</li>{/if}
    </ul>{/if}
  {#snippet footer()}<span>{item.status}</span>{/snippet}
</CardFrame>

<style>
  .outputs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .outputs li {
    max-inline-size: 100%;
    padding: 3px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font-size: var(--text-caption);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .outputs .more {
    color: var(--color-faint);
  }
</style>

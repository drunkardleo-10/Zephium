<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { displayHost } from "$shared/ui/data/Artifact";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const host = $derived(displayHost(item.subject?.homepage));
</script>

<CardFrame kind={item.kind} title={item.title} {selected} unavailable={item.unavailable} dense>
  {#snippet leading()}<span class="mark" aria-hidden="true">{item.title.slice(0, 1)}</span
    >{/snippet}
  {#if item.detail}<p class="descriptor">{item.detail}</p>{/if}
  {#snippet footer()}<span>{host || item.status}</span>{/snippet}
</CardFrame>

<style>
  .mark {
    display: grid;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-sm);
    background: var(--color-accent-soft);
    color: var(--color-text);
    font-weight: 700;
    text-transform: uppercase;
  }

  .descriptor {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }
</style>

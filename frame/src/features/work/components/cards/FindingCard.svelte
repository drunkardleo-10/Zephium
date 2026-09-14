<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { Tick02Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const confidence = $derived(item.finding?.confidence ?? "unverified");
</script>

<CardFrame
  kind={item.kind}
  title={item.title}
  icon={Tick02Icon}
  {selected}
  unavailable={item.unavailable}
>
  {#if item.detail}<p class="detail">{item.detail}</p>{/if}
  {#snippet footer()}<span class={`confidence ${confidence}`}>{confidence}</span><span
      >{item.finding?.evidence.length ?? 0}</span
    >{/snippet}
</CardFrame>

<style>
  .detail {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .confidence {
    text-transform: capitalize;
  }

  .confidence.supported {
    color: var(--color-success);
  }

  .confidence.contradicted {
    color: var(--color-danger);
  }
</style>

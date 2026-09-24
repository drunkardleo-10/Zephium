<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { Tick02Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const confidence = $derived(item.finding?.confidence ?? "unverified");
</script>

<!-- A claim the person placed on its own; the run folds its claims into one Findings card. -->
<CardFrame title={item.title} icon={Tick02Icon} {selected} unavailable={item.unavailable} dense>
  {#if item.detail}<p class="detail">{item.detail}</p>{/if}
  {#snippet footer()}<span class={`confidence ${confidence}`}
      ><span class="dot" aria-hidden="true"></span>{item.kind}</span
    ><span class="count">{item.finding?.evidence.length ?? 0}</span>{/snippet}
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
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    inline-size: 6px;
    block-size: 6px;
    border-radius: 50%;
    background: var(--color-faint);
  }

  .supported .dot {
    background: var(--color-success);
  }

  .contradicted .dot {
    background: var(--color-danger);
  }

  .count {
    font-variant-numeric: tabular-nums;
  }
</style>

<script lang="ts">
  import { Handle, Position, type NodeProps, type Node } from "@xyflow/svelte";
  import { getContext, onMount } from "svelte";
  import { canvasArrival } from "../lib/canvas-context";
  import { arrive } from "../lib/arrival";
  import type { Duration } from "$shared/lib/motion";
  import type { ClusterData } from "../lib/canvas-model";
  let { id, data }: NodeProps<Node<ClusterData, "cluster">> = $props();
  const arrival = getContext<((id: string) => Duration | null) | undefined>(canvasArrival);
  let root = $state<HTMLDivElement>();
  // A group the run just made rises into place; one that was already there does not.
  onMount(() => {
    const motion = arrival?.(id);
    if (motion && root?.parentElement) arrive(root.parentElement, motion);
  });
</script>

<Handle
  type="target"
  position={Position.Left}
  isConnectable={false}
  tabindex={-1}
  aria-hidden="true"
/>
<div class="cluster" class:active={data.active} style:--inset="{data.inset}px" bind:this={root}>
  <span class="label"
    >{data.label}{#if data.more}<span class="more">+{data.more}</span>{/if}</span
  >
</div>
<Handle
  type="source"
  position={Position.Right}
  isConnectable={false}
  tabindex={-1}
  aria-hidden="true"
/>

<style>
  .cluster {
    position: relative;
    box-sizing: border-box;
    block-size: 100%;
    border-radius: var(--radius-panel);
    background: var(--row-hover);
    box-shadow: inset 0 0 0 1px var(--color-border);
    transition: box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .cluster.active {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .label {
    position: absolute;
    inset-block-start: var(--inset);
    inset-inline-start: var(--inset);
    display: flex;
    align-items: center;
    gap: 6px;
    block-size: 20px;
    padding-block-end: 4px;
    box-sizing: border-box;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .more {
    padding: 0 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
  }
</style>

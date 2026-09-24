<script lang="ts">
  import { Handle, Position, type NodeProps, type Node } from "@xyflow/svelte";
  import type { ClusterData } from "../lib/canvas-model";
  let { data }: NodeProps<Node<ClusterData, "cluster">> = $props();
</script>

<Handle
  type="target"
  position={Position.Left}
  isConnectable={false}
  tabindex={-1}
  aria-hidden="true"
/>
<div class="cluster" class:active={data.active}>
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
    border-radius: var(--radius-card);
    box-shadow: inset 0 0 0 1px transparent;
    transition: box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .cluster.active {
    box-shadow: inset 0 0 0 1px var(--color-border);
  }

  .label {
    position: absolute;
    inset-block-end: 100%;
    inset-inline-start: 12px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding-block-end: 4px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .more {
    padding: 0 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
  }
</style>

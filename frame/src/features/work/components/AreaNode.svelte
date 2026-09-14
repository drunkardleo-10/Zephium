<script lang="ts">
  import { NodeResizer, type NodeProps, type Node } from "@xyflow/svelte";
  import { getContext } from "svelte";
  import { canvasResize } from "../lib/canvas-context";
  import type { AreaData } from "../lib/canvas-model";
  const resize = getContext<(active: boolean) => void>(canvasResize);
  let { data, selected }: NodeProps<Node<AreaData, "area">> = $props();
</script>

<NodeResizer
  onResizeStart={() => resize(true)}
  onResizeEnd={() => resize(false)}
  isVisible={selected}
  minWidth={240}
  minHeight={160}
  maxWidth={8192}
  maxHeight={8192}
  lineClass="work-resize-line"
  handleClass="work-resize-handle"
/>
<section class="area" class:selected aria-label={data.title}>
  <header class="area-title">
    <span class="name">{data.title}</span>
    {#if data.count}<span class="count">{data.count}</span>{/if}
  </header>
</section>

<style>
  .area {
    box-sizing: border-box;
    block-size: 100%;
    border-radius: var(--radius-card);
    background: color-mix(in srgb, var(--color-fill) 60%, transparent);
    box-shadow: inset 0 0 0 1px var(--color-border);
    transition: box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .area.selected {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      0 0 0 2px var(--color-accent-soft);
  }

  .area-title {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    color: var(--color-muted);
    cursor: grab;
  }

  .area-title:active {
    cursor: grabbing;
  }

  .name {
    font-size: var(--text-label);
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .count {
    padding: 0 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }
</style>

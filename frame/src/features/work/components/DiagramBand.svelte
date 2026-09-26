<script lang="ts">
  import type { NodeProps, Node } from "@xyflow/svelte";
  import type { BandData } from "../lib/canvas-model";
  let { data }: NodeProps<Node<BandData, "band">> = $props();
</script>

<!-- A layer's swimlane: a soft band behind its parts, captioned at its top-left. -->
<div class="band"><span class="caption">{data.name}</span></div>

<style>
  .band {
    box-sizing: border-box;
    block-size: 100%;
    padding: 8px 12px;
    border-radius: var(--radius-panel);
    background: color-mix(in oklab, var(--color-fill) 70%, transparent);
    box-shadow: inset 0 1px 0 var(--color-border);
  }

  .caption {
    display: block;
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
    line-height: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>

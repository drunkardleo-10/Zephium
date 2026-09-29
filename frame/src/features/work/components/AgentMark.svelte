<script lang="ts">
  import type { NodeProps, Node } from "@xyflow/svelte";
  import type { CanvasItem } from "../lib/canvas-model";
  let { data }: NodeProps<Node<CanvasItem, "agent">> = $props();
</script>

<!--
  Where the agent works: an anchor the view follows, drawn by nothing. The
  part at work says what it does and its live page is the live thing; no orb
  stands in for them.
-->
<div class="mark" role="status" aria-label={data.status}></div>

<style>
  .mark {
    inline-size: 24px;
    block-size: 24px;
    pointer-events: none;
  }
</style>

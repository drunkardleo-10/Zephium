<script lang="ts">
  import type { NodeProps, Node } from "@xyflow/svelte";
  import AgentAvatar from "./cards/AgentAvatar.svelte";
  import type { CanvasItem } from "../lib/canvas-model";
  let { data }: NodeProps<Node<CanvasItem, "agent">> = $props();
  const caption = $derived(data.agent?.caption ?? "");
</script>

<div class="mark" role="status" aria-label={data.status}>
  <span class="orb"><AgentAvatar seed={data.agent?.seed ?? 0} size={24} /></span>
  <span class="caption" class:shown={!!caption} aria-hidden="true">{caption}</span>
</div>

<style>
  .mark {
    position: relative;
    inline-size: 24px;
    block-size: 24px;
    pointer-events: none;
  }

  .orb {
    display: block;
    inline-size: 24px;
    block-size: 24px;
    border-radius: var(--radius-capsule);
    box-shadow:
      0 0 0 1px var(--color-lit),
      var(--shadow-raised);
  }

  .caption {
    position: absolute;
    inset-block-start: 50%;
    inset-inline-start: calc(100% + 8px);
    translate: 0 -50%;
    padding: 3px 9px;
    border-radius: var(--radius-capsule);
    background: var(--color-float);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
    font-size: var(--text-caption);
    font-weight: 500;
    line-height: 14px;
    white-space: nowrap;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .caption.shown {
    opacity: 1;
  }

  .caption:empty {
    display: none;
  }
</style>

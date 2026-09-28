<script lang="ts">
  import type { NodeProps, Node } from "@xyflow/svelte";
  import AgentAvatar from "./cards/AgentAvatar.svelte";
  import { getContext } from "svelte";
  import type { CanvasItem } from "../lib/canvas-model";
  import type { Detail } from "../lib/board/types";
  import { canvasDetail } from "../lib/canvas-context";
  let { data }: NodeProps<Node<CanvasItem, "agent">> = $props();
  const zoom = getContext<{ readonly level: Detail } | undefined>(canvasDetail);
  const caption = $derived(data.agent?.caption ?? "");
</script>

<div class="mark {zoom?.level ?? 'full'}" role="status" aria-label={data.status}>
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

  /* From afar the orb and its words stay the size a person reads them at. */
  .overview .orb {
    scale: 1.75;
  }

  .overview .caption {
    inset-inline-start: calc(100% + 20px);
    padding: 6px 16px;
    font-size: 22px;
    line-height: 28px;
  }

  .tile .orb {
    scale: 2.5;
  }

  .tile .caption {
    display: none;
  }

  .caption:empty {
    display: none;
  }
</style>

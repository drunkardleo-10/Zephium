<script lang="ts">
  import AgentAvatar from "./AgentAvatar.svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  /** The canvas moves the capsule over 700 ms; while it walks nothing else on it moves. */
  const WALK_MS = 700;
  let walking = $state(false);
  let from = "";
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const stand = JSON.stringify(item.agent?.stand ?? null);
    const moved = from !== "" && stand !== from;
    from = stand;
    if (!moved) return;
    walking = true;
    clearTimeout(timer);
    timer = setTimeout(() => (walking = false), WALK_MS);
  });
  $effect(() => () => clearTimeout(timer));
  const active = $derived(!!item.agent && item.agent.activity !== "" && !walking);
</script>

<article class="agent" class:selected>
  <div class="work-drag-handle grip"></div>
  <AgentAvatar seed={item.agent?.seed ?? 0} {active} />
  <div class="text">
    <span class="kind">{item.kind}</span>
    <strong>{item.status}</strong>
    {#if item.agent?.line}<span class="line">{item.agent.line}</span>{/if}
  </div>
</article>

<style>
  .agent {
    display: flex;
    align-items: center;
    gap: 12px;
    box-sizing: border-box;
    block-size: 100%;
    padding: 12px 16px;
    border-radius: var(--radius-capsule);
    background: var(--color-menu);
    backdrop-filter: blur(10px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    color: var(--color-text);
    position: relative;
    overflow: hidden;
  }

  .agent.selected {
    box-shadow:
      var(--shadow-popover),
      0 0 0 2px var(--color-accent-soft);
  }

  .grip {
    position: absolute;
    inset: 0;
    cursor: grab;
  }

  .text {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-inline-size: 0;
    pointer-events: none;
  }

  .kind {
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  strong {
    font-size: var(--text-body);
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .line {
    color: var(--color-muted);
    font-size: var(--text-caption);
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
</style>

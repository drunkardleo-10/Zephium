<script lang="ts">
  import { untrack } from "svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  // A frame that missed once must not pin the placeholder: the store serves it
  // again once the run settles, and every refresh mints a new generation.
  let failedFrame = $state<string | null>(null);
  const frame = $derived(item.page?.frame ?? null);
  const failed = $derived(frame !== null && failedFrame === frame);
  $effect(() => {
    const current = frame;
    untrack(() => {
      if (failedFrame !== null && failedFrame !== current) failedFrame = null;
    });
  });
</script>

<article class="page" class:selected class:live={item.page?.live}>
  <header class="work-drag-handle">
    <span class="host">{item.page?.host || item.kind}</span>
    <span class="state">
      {#if item.page?.live}<span class="dot" aria-hidden="true"></span>{/if}{item.status}
    </span>
  </header>
  <div class="frame">
    {#if frame && !failed}
      <img src={frame} alt={item.title} draggable="false" onerror={() => (failedFrame = frame)} />
    {:else}
      <span class="placeholder" aria-hidden="true">{(item.page?.host || "?").slice(0, 1)}</span>
    {/if}
  </div>
</article>

<style>
  .page {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    block-size: 100%;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    box-shadow: var(--shadow-popover);
    color: var(--color-text);
    overflow: hidden;
  }

  .page.selected {
    box-shadow:
      var(--shadow-popover),
      0 0 0 2px var(--color-accent-soft);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 12px;
    font-size: var(--text-label);
    cursor: grab;
  }

  .host {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }

  .state {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--color-muted);
    white-space: nowrap;
  }

  .dot {
    inline-size: 6px;
    block-size: 6px;
    border-radius: 50%;
    background: var(--color-accent);
    animation: pulse 1.6s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .frame {
    position: relative;
    flex: 1;
    min-block-size: 0;
    margin: 0 8px 8px;
    border-radius: var(--radius-sm);
    background: var(--color-fill);
    overflow: hidden;
    display: grid;
    place-items: center;
  }

  .frame img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    object-position: top;
    pointer-events: none;
  }

  .placeholder {
    font-size: 28px;
    font-weight: 700;
    color: var(--color-faint);
    text-transform: uppercase;
  }

  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }
</style>

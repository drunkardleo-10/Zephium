<script lang="ts">
  import { getContext } from "svelte";
  import { canvasAuthor } from "../../lib/canvas-context";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected, onaction }: { item: CanvasItem; selected: boolean; onaction: () => void } =
    $props();
  const author = getContext<{ readonly initial: string } | undefined>(canvasAuthor);
</script>

<article class="request" class:selected>
  <div class="work-drag-handle grip"></div>
  <span class="you" title={m.work_env_you()}>{author?.initial || "•"}</span>
  <div class="text">
    <p class="sentence">{item.title}</p>
    <!-- Only a plan objective carries an action; a request says its words and nothing else. -->
    {#if item.actionLabel}<button
        type="button"
        class="link nodrag nopan"
        onclick={(event) => {
          event.stopPropagation();
          onaction();
        }}>{item.actionLabel}</button
      >{/if}
  </div>
</article>

<style>
  /* The person's words, on the one raised surface no machine card uses. */
  .request {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    box-sizing: border-box;
    block-size: 100%;
    padding: 14px 16px;
    border-radius: var(--radius-lg);
    background: var(--color-raised);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-text);
    overflow: hidden;
    transition: box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .request:hover {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .request.selected {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      0 0 0 2px var(--color-accent-soft),
      0 0 0 3px var(--color-accent);
  }

  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__node.dragging) .request {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      var(--shadow-popover);
  }

  .grip {
    position: absolute;
    inset: 0;
    cursor: grab;
  }

  .grip:active {
    cursor: grabbing;
  }

  .you {
    position: relative;
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 24px;
    block-size: 24px;
    border-radius: 50%;
    background: var(--color-fill-active);
    color: var(--color-label-secondary);
    font-size: var(--text-caption);
    font-weight: 600;
    pointer-events: none;
  }

  .text {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    flex: 1;
    min-inline-size: 0;
    padding-block-start: 3px;
  }

  .sentence {
    position: relative;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 17px;
    letter-spacing: -0.005em;
    text-wrap: pretty;
    overflow-wrap: anywhere;
    pointer-events: none;
  }

  .link {
    position: relative;
    border: 0;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  .link:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>

<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected, onaction }: { item: CanvasItem; selected: boolean; onaction: () => void } =
    $props();
</script>

<!-- The person's words: what it is, then the sentence whole. -->
<CardFrame id={item.id} {selected} plain>
  <div class="request">
    <p class="caption">{item.kind}</p>
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
</CardFrame>

<style>
  .request {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    box-sizing: border-box;
    block-size: 100%;
    min-block-size: 0;
    padding: 8px 12px;
  }

  .caption {
    margin: 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 15px;
  }

  .sentence {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 8;
    line-clamp: 8;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 17px;
    letter-spacing: -0.005em;
    text-wrap: pretty;
    overflow-wrap: anywhere;
  }

  .link {
    margin-block-start: 8px;
    padding: 2px 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-control);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .link:hover {
    background: var(--color-control-hover);
  }

  .link:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>

<script lang="ts">
  import type { CanvasItem } from "../../lib/canvas-model";
  import type { Detail } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";

  let {
    item,
    selected,
    detail = "full",
    ontoggle,
    onaction,
  }: {
    item: CanvasItem;
    selected: boolean;
    detail?: Detail;
    ontoggle: () => void;
    /** The request's one action, when it has one, such as showing its plan. */
    onaction?: () => void;
  } = $props();

  let words = $state<HTMLElement>();
  let clipped = $state(false);
  $effect(() => {
    void item.title;
    void item.expanded;
    const element = words;
    if (!element) return;
    const measure = () => (clipped = element.scrollHeight > element.clientHeight + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  });
</script>

<!--
  The person's words, as words: no card around them. Who and when above; two
  lines at rest, and a click opens every line where they stand.
-->
<div
  class="request work-drag-handle {detail}"
  class:selected
  data-work-request={item.id}
  data-card-id={item.id}
>
  <p class="meta">
    <span>{m.work_request_you()}</span>{#if item.when}<span class="dot" aria-hidden="true">·</span
      ><time>{item.when}</time>{/if}
  </p>
  <p class="words" class:open={item.expanded} bind:this={words}>{item.title}</p>
  {#if clipped || item.expanded || (item.actionLabel && onaction)}<p class="actions">
      {#if clipped || item.expanded}<button
          type="button"
          class="more nodrag nopan"
          aria-expanded={!!item.expanded}
          onclick={(event) => {
            event.stopPropagation();
            ontoggle();
          }}>{item.expanded ? m.work_request_less() : m.work_request_more()}</button
        >{/if}
      {#if item.actionLabel && onaction}<button
          type="button"
          class="more nodrag nopan"
          onclick={(event) => {
            event.stopPropagation();
            onaction();
          }}>{item.actionLabel}</button
        >{/if}
    </p>{/if}
</div>

<style>
  .request {
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    padding: 0 4px;
    border-radius: var(--radius-control);
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .request.selected {
    box-shadow: 0 0 0 1.5px var(--color-ring);
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0 0 4px;
    color: var(--color-faint);
    font-size: var(--text-label);
    font-weight: 500;
    line-height: 16px;
  }

  .dot {
    opacity: 0.7;
  }

  time {
    font-variant-numeric: tabular-nums;
  }

  .words {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    color: var(--color-text);
    font-size: 17px;
    font-weight: 500;
    letter-spacing: -0.012em;
    line-height: 24px;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow-wrap: anywhere;
  }

  .words.open {
    display: block;
    overflow: visible;
    -webkit-line-clamp: unset;
    line-clamp: unset;
  }

  .actions {
    display: flex;
    gap: 14px;
    margin: 4px 0 0;
  }

  .more {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
  }

  .more:hover {
    color: var(--color-text);
  }

  .more:focus-visible {
    border-radius: var(--radius-inset);
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  /* Surveyed from afar: the words alone, set to read at half size and below. */
  .overview .meta,
  .overview .actions,
  .tile .meta,
  .tile .actions {
    display: none;
  }

  .overview .words {
    margin-block-start: 15px;
    font-size: 28px;
    font-weight: 600;
    letter-spacing: -0.02em;
    line-height: 34px;
  }

  .tile .words {
    margin-block-start: 6px;
    font-size: 44px;
    font-weight: 600;
    letter-spacing: -0.025em;
    line-height: 52px;
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }
</style>

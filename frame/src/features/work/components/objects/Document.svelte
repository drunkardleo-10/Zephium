<script lang="ts">
  import DocumentView from "$shared/ui/data/Artifact/DocumentView.svelte";
  import * as m from "$shared/i18n/messages";
  import type { DocumentObjectView, ObjectActions } from "../../lib/board/types";
  /** Text the person asked for, set as a sheet of paper: its first page on the canvas. */
  let {
    object,
    actions = {},
    centre = false,
  }: {
    object: DocumentObjectView;
    actions?: ObjectActions;
    /** Opened in the centre: all of it, at reading size. */
    centre?: boolean;
  } = $props();
  let body = $state<HTMLElement>();
  let width = $state(0);
  let overflows = $state(false);
  $effect(() => {
    const element = body;
    if (!element) return;
    const check = () => (overflows = element.scrollHeight > element.clientHeight + 1);
    check();
    const observer = new ResizeObserver(check);
    observer.observe(element);
    return () => observer.disconnect();
  });
</script>

<article class="paper" aria-label={object.title}>
  <div
    class="page"
    class:overflows
    bind:this={body}
    bind:clientWidth={width}
    style:max-block-size={width && !centre ? `${Math.round(width * 1.294)}px` : undefined}
    style:min-block-size={width ? `${Math.round(width * 0.72)}px` : undefined}
  >
    {#if object.title}<h2>{object.title}</h2>{/if}
    <div class="text">
      {#if object.content.formatted}<DocumentView
          document={object.content.formatted}
          onlink={actions.link}
        />{:else}{#each object.content.paragraphs as paragraph, index (index)}<p>
            {paragraph}
          </p>{/each}{/if}
    </div>
  </div>
  {#if overflows && !centre}<button
      type="button"
      class="read nodrag nopan"
      onclick={() => actions.open?.(object.id)}>{m.work_object_read_all()}</button
    >{/if}
</article>

<style>
  .paper {
    position: relative;
    box-sizing: border-box;
    inline-size: 100%;
    border-radius: var(--radius-inset);
    background: var(--color-raised);
    box-shadow: var(--shadow-float);
    color: var(--color-text);
  }

  .page {
    box-sizing: border-box;
    padding: 44px 48px;
    overflow: hidden;
  }

  .page.overflows {
    mask-image: var(--mask-fade-bottom);
  }

  h2 {
    margin: 0 0 18px;
    font-size: var(--text-title);
    font-weight: 700;
    line-height: 1.2;
    letter-spacing: -0.02em;
    text-wrap: balance;
  }

  .text {
    max-inline-size: 62ch;
    color: var(--color-label-secondary);
    font-size: var(--text-page-title);
    line-height: 1.6;
    hyphens: auto;
    text-wrap: pretty;
  }

  .text :global(p) {
    margin: 0 0 12px;
  }

  .text :global(h3),
  .text :global(h4) {
    margin: 22px 0 8px;
    color: var(--color-text);
    font-size: calc(var(--text-page-title) + 2px);
    font-weight: 650;
    letter-spacing: -0.01em;
  }

  .text :global(h3:first-child),
  .text :global(h4:first-child) {
    margin-block-start: 0;
  }

  .text :global(ul),
  .text :global(ol) {
    margin: 0 0 12px;
    padding-inline-start: 22px;
  }

  .text :global(ul) {
    list-style: disc;
  }

  .text :global(ol) {
    list-style: decimal;
  }

  .text :global(li) {
    margin-block-end: 4px;
  }

  .text :global(li::marker) {
    color: var(--color-faint);
  }

  .read {
    position: absolute;
    inset-block-end: 16px;
    inset-inline-start: 48px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 600;
    cursor: default;
  }

  .read:hover {
    text-decoration: underline;
    text-underline-offset: 3px;
  }
</style>

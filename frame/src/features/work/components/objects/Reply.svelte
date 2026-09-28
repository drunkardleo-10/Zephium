<script lang="ts">
  import type { Detail, ReplyView } from "../../lib/board/types";
  import Inline from "./Inline.svelte";
  /** The answer, set on the canvas like a caption: a headline, a few lines, the figures. */
  let { object, detail }: { object: ReplyView; detail: Detail } = $props();
</script>

<article class="reply {detail}" aria-label={object.headline}>
  <h2>{object.headline}</h2>
  {#if detail === "full" && object.text}<p class="text"><Inline text={object.text} /></p>{/if}
  {#if detail !== "tile" && object.figures.length}
    <dl class="figures">
      {#each object.figures as figure (figure.label)}
        <div>
          <dt>{figure.label}</dt>
          <dd>
            <span class="value">{figure.value}</span>{#if figure.note && detail === "full"}<span
                class="note">{figure.note}</span
              >{/if}
          </dd>
        </div>
      {/each}
    </dl>
  {/if}
  {#if detail === "full" && object.points.length}
    <ul class="points">
      {#each object.points as point (point)}<li><Inline text={point} /></li>{/each}
    </ul>
  {/if}
</article>

<style>
  .reply {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-inline-size: 64ch;
    color: var(--color-text);
  }

  h2 {
    margin: 0;
    font-size: var(--text-headline);
    font-weight: 650;
    line-height: 1.18;
    letter-spacing: -0.022em;
    text-wrap: balance;
  }

  .text {
    margin: 0;
    color: var(--color-label-secondary);
    font-size: var(--text-reading);
    line-height: 1.55;
    text-wrap: pretty;
  }

  .figures {
    display: flex;
    flex-wrap: wrap;
    gap: 16px 40px;
    margin: 6px 0 0;
  }

  .figures div {
    display: flex;
    flex-direction: column-reverse;
    gap: 4px;
  }

  dt {
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  dd {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
  }

  .value {
    font-size: var(--text-headline);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
    line-height: 1.1;
  }

  .note {
    order: 2;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .points {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 2px 0 0;
    padding: 0;
    list-style: none;
    color: var(--color-label-secondary);
    font-size: var(--text-reading);
    line-height: 1.45;
  }

  .points li {
    position: relative;
    padding-inline-start: 18px;
  }

  .points li::before {
    position: absolute;
    inset-block-start: 0.62em;
    inset-inline-start: 2px;
    inline-size: 5px;
    block-size: 5px;
    border-radius: var(--radius-capsule);
    background: var(--color-faint);
    content: "";
  }

  .overview {
    gap: 24px;
  }

  .overview h2 {
    font-size: var(--text-overview-figure);
  }

  .overview dt {
    font-size: var(--text-overview-label);
  }

  .overview .value {
    font-size: var(--text-overview-figure);
  }

  .overview .figures {
    gap: 24px 56px;
  }

  .tile h2 {
    font-size: var(--text-tile-title);
  }
</style>

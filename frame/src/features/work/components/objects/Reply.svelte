<script lang="ts">
  import type { ReplyView } from "../../lib/board/types";
  import DocumentView from "$shared/ui/data/Artifact/DocumentView.svelte";
  import Inline from "./Inline.svelte";
  import Orb from "$shared/ui/presence/Orb.svelte";
  import Shimmer from "$shared/ui/presence/Shimmer.svelte";
  /** The answer, set on the canvas like a caption: a headline, a few lines, the figures. */
  let {
    object,
    centre = false,
  }: {
    object: ReplyView;
    /** Opened in the centre: an older answer's remaining paragraphs read on below. */
    centre?: boolean;
  } = $props();
</script>

<article class="reply" class:waiting={object.state === "pending"} aria-label={object.headline}>
  {#if object.state === "pending"}
    <!-- Still coming: the working indicator and a line of light, never set as the answer's headline. -->
    <p class="pending" role="status">
      <Orb size={22} /><span class="coming"><Shimmer text={object.headline} /></span>
    </p>
  {:else}
    <h2>{object.headline}</h2>
    {#if object.text}<p class="text"><Inline text={object.text} /></p>{/if}
    {#if object.figures.length}
      <dl class="figures">
        {#each object.figures as figure (figure.label)}
          <div>
            <dt>{figure.label}</dt>
            <dd>
              <span class="value">{figure.value}</span>{#if figure.note}<span class="note"
                  >{figure.note}</span
                >{/if}
            </dd>
          </div>
        {/each}
      </dl>
    {/if}
    {#if object.points.length}
      <ul class="points">
        {#each object.points as point (point)}<li><Inline text={point} /></li>{/each}
      </ul>
    {/if}
  {/if}
  {#if centre && object.more?.length}<div class="more">
      <DocumentView
        document={{ version: 1, document: { type: "doc", content: [...object.more] } }}
      />
    </div>{/if}
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
    flex-direction: column;
    gap: 3px;
  }

  dt {
    order: 2;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  dd {
    display: contents;
  }

  .value {
    order: 1;
    font-size: var(--text-headline);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
    line-height: 1.1;
  }

  .note {
    order: 3;
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

  .pending {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
    line-height: 1.2;
    letter-spacing: -0.016em;

    --shimmer-rest: var(--color-faint);
    --shimmer-lit: var(--color-text);
  }

  .coming {
    min-inline-size: 0;
  }

  .more {
    color: var(--color-label-secondary);
    font-size: var(--text-reading);
    line-height: 1.55;
  }
</style>

<script lang="ts">
  import type { Detail, ObjectActions, PageObjectView } from "../../lib/board/types";
  import Mark from "./Mark.svelte";
  import { host } from "./sheet";
  /** A page as its captured frame under its own title bar; without one, its title set large. */
  let {
    object,
    detail,
    actions = {},
  }: { object: PageObjectView; detail: Detail; actions?: ObjectActions } = $props();
  /** The capture that would not load; a newer frame gets its own chance. */
  let broken = $state<string | null>(null);
  const frame = $derived(object.frame && broken !== object.frame ? object.frame : null);
  const site = $derived(host(object.url));
</script>

<article class="page {detail}" class:live={object.live} aria-label={object.title}>
  {#if detail !== "tile"}
    <header>
      <Mark address={object.url} size={detail === "full" ? 14 : 28} />
      <span class="title">{object.title}</span>
    </header>
  {/if}
  <button
    type="button"
    class="view nodrag nopan"
    aria-label={object.title}
    onclick={() => (actions.open ? actions.open(object.id) : actions.link?.(object.url))}
  >
    {#if frame}<img
        src={frame}
        alt=""
        decoding="async"
        loading="lazy"
        draggable="false"
        onerror={() => (broken = object.frame ?? null)}
      />{:else}<span class="blank">
        <span class="sign"><Mark address={object.url} size={detail === "full" ? 28 : 56} /></span>
        <span class="name">{object.title}</span>
        <span class="host">{site}</span>
      </span>{/if}
  </button>
</article>

<style>
  .page {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    inline-size: 100%;
    overflow: hidden;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
  }

  .page.live {
    box-shadow:
      0 0 0 1.5px var(--color-lit),
      var(--shadow-raised);
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
    padding: 9px 12px;
    font-size: var(--text-label);
  }

  .title {
    display: -webkit-box;
    min-inline-size: 0;
    overflow: hidden;
    font-weight: 550;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }

  .view {
    display: block;
    aspect-ratio: 16 / 10;
    padding: 0;
    overflow: hidden;
    border: 0;
    background: var(--color-raised);
    cursor: default;
  }

  .view img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    object-position: top;
  }

  /* The capture failed: the page still reads as itself, by its title and its site. */
  .blank {
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 6px;
    box-sizing: border-box;
    block-size: 100%;
    padding: 28px;
    text-align: start;
  }

  .sign {
    margin-block-end: auto;
  }

  .name {
    color: var(--color-text);
    font-size: var(--text-title);
    font-weight: 650;
    line-height: 1.2;
    letter-spacing: -0.015em;
    text-wrap: balance;
  }

  .host {
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .overview header {
    gap: 14px;
    padding: 16px 22px;
    font-size: var(--text-overview-label);
  }

  .overview .name {
    font-size: var(--text-overview-figure);
  }

  .overview .host {
    font-size: var(--text-overview-label);
  }

  .tile .name {
    font-size: var(--text-tile-title);
  }

  .tile .host {
    display: none;
  }
</style>

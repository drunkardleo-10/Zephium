<script lang="ts">
  import type { BranchPage, CanvasItem } from "../../lib/canvas-model";
  import { reasonBadge } from "../../lib/work-human";
  import HostGlyph from "./HostGlyph.svelte";
  import * as m from "$shared/i18n/messages";

  let {
    item,
    selected,
    onopen,
  }: { item: CanvasItem; selected: boolean; onopen: (page: string) => void } = $props();

  const SHOWN = 3;
  const host = $derived(item.branch?.host ?? item.title);
  const pages = $derived(item.branch?.pages ?? []);
  const live = $derived(!!item.branch?.live);
  /** The page it is on now leads, then the newest; the rest stay counted behind the last tile. */
  const shown = $derived.by(() => {
    const ordered = [...pages].reverse().sort((a, b) => Number(b.live) - Number(a.live));
    return ordered.slice(0, SHOWN);
  });
  const hidden = $derived(Math.max(0, pages.length - SHOWN));
  const waiting = $derived(pages.find((page) => page.human?.phase === "waiting_for_human"));
  /** Frames that failed once show the site's mark until a newer frame arrives. */
  let broken = $state<Record<string, string>>({});
</script>

{#snippet tile(page: BranchPage, index: number)}
  {@const stacked = hidden > 0 && index === shown.length - 1}
  <button
    type="button"
    class="tile page nodrag"
    class:live={page.live}
    class:stacked
    title={page.title}
    aria-label={page.title}
    onclick={(event) => {
      event.stopPropagation();
      onopen(page.id);
    }}
  >
    <span class="glass">
      {#if page.frame && broken[page.id] !== page.frame}
        <img
          src={page.frame}
          alt=""
          draggable="false"
          decoding="async"
          loading="lazy"
          onerror={() => (broken = { ...broken, [page.id]: page.frame ?? "" })}
        />
      {:else}
        <span class="mark"><HostGlyph {host} url={page.url} size={26} initial={false} /></span>
      {/if}
      {#if page.human?.phase === "waiting_for_human"}<span class="needs"
          ><span class="why">{reasonBadge(page.human.reason)}</span><span
            class="help"
            role="presentation">{m.work_human_help()}</span
          ></span
        >{/if}
      {#if stacked}<span class="more">+{hidden}</span>{/if}
    </span>
    <span class="caption">{page.tab ? page.status : page.title}</span>
  </button>
{/snippet}

<!--
  One place the agent worked: the site, then the pages it opened there as
  what they looked like. No card around it; the pages are the object.
-->
<section class="branch work-drag-handle" class:selected aria-label={host}>
  <header>
    <HostGlyph {host} size={16} loading={live} initial={false} />
    <strong>{host}</strong>
    <span class="count"
      >{live
        ? m.work_branch_working()
        : pages.length === 1
          ? m.work_branch_pages_one()
          : m.work_branch_pages({ count: pages.length })}</span
    >
    {#if waiting}<span class="turn">{m.work_line_waiting_for_you()}</span>{/if}
  </header>
  <div class="row">
    {#each shown as page, index (page.id)}{@render tile(page, index)}{/each}
  </div>
</section>

<style>
  .branch {
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    border-radius: var(--radius-card);
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .branch.selected {
    box-shadow: 0 0 0 1.5px var(--color-ring);
  }

  header {
    display: flex;
    align-items: center;
    gap: 7px;
    min-inline-size: 0;
    overflow: hidden;
    white-space: nowrap;
    block-size: 24px;
    padding-inline: 2px;
    color: var(--color-text);
    font-size: 13px;
  }

  strong {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
  }

  .count {
    flex: none;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .turn {
    margin-inline-start: auto;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-accent-soft);
    color: var(--color-text);
    font-size: var(--text-caption);
    font-weight: 600;
  }

  .row {
    display: grid;
    grid-auto-columns: minmax(0, 1fr);
    grid-auto-flow: column;
    gap: 10px;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-inline-size: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .glass {
    position: relative;
    display: grid;
    aspect-ratio: 16 / 10;
    overflow: hidden;
    border-radius: var(--radius-control);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
    place-items: center;
    transition:
      box-shadow var(--motion-base) var(--ease-out),
      translate var(--motion-base) var(--ease-spring);
  }

  /* Pages beyond the row are counted behind the last: two edges show under it. */
  .tile.stacked .glass {
    box-shadow:
      var(--shadow-raised),
      4px 4px 0 -1px var(--color-surface),
      8px 8px 0 -2px color-mix(in srgb, var(--color-surface) 70%, transparent);
  }

  .tile:hover .glass {
    translate: 0 -2px;
  }

  .tile.live .glass {
    box-shadow:
      0 0 0 1.5px var(--color-accent),
      var(--shadow-raised);
  }

  .tile:focus-visible .glass {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    object-position: top;
    pointer-events: none;
  }

  .mark {
    display: grid;
    place-items: center;
    opacity: 0.8;
  }

  /* The person's turn: what it needs, and the one way to give it. */
  .needs {
    position: absolute;
    inset: auto 6px 6px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 4px 4px 4px 8px;
    border-radius: var(--radius-control-compact);
    background: var(--color-float);
    box-shadow: var(--shadow-menu);
    color: var(--color-text);
    font-size: var(--text-caption);
    font-weight: 600;
    line-height: 14px;
  }

  .help {
    padding: 3px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  .more {
    position: absolute;
    inset-block-start: 6px;
    inset-inline-end: 6px;
    padding: 2px 7px;
    border-radius: var(--radius-capsule);
    background: var(--color-float);
    box-shadow: var(--shadow-menu);
    color: var(--color-text);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  .caption {
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>

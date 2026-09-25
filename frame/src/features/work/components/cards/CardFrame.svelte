<script lang="ts">
  import type { Snippet } from "svelte";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "$shared/ui/Icon";
  import { provideSiteMarks } from "$shared/ui/data/Artifact/site-marks";
  import { siteMark } from "./HostGlyph.svelte";
  let {
    id,
    kind = "",
    title = "",
    icon,
    leading,
    aside,
    hero,
    count,
    lines = 2,
    mono = false,
    selected = false,
    active = false,
    unavailable = false,
    dense = false,
    row = false,
    plain = false,
    children,
    footer,
  }: {
    /** The canvas item this card draws; the lift finds its source by it. */
    id?: string;
    /** Said only where the card does not already say what it is. */
    kind?: string;
    title?: string;
    icon?: IconSvgElement;
    /** A mark in place of the icon plate: a favicon, a strip of them. */
    leading?: Snippet;
    /** The header's other end: an elapsed time, a count. */
    aside?: Snippet;
    /** A picture above the header. */
    hero?: Snippet;
    /** A tally beside the title, in tabular numerals. */
    count?: number;
    lines?: 1 | 2;
    mono?: boolean;
    selected?: boolean;
    /** The stage a run is working in right now. */
    active?: boolean;
    unavailable?: boolean;
    dense?: boolean;
    /** A local thing (folder, file, command): its header is a row on main's row plate. */
    row?: boolean;
    /** The card draws its own layout inside the frame. */
    plain?: boolean;
    children?: Snippet;
    footer?: Snippet;
  } = $props();
  // Evidence chips and source rows inside a card draw the sites' real marks.
  provideSiteMarks(siteMark);
</script>

<!-- The whole card carries it; controls inside opt out with nodrag. -->
<article
  class="card work-drag-handle"
  data-card-id={id}
  class:selected
  class:active
  class:unavailable
  class:dense
  class:row
  class:plain
>
  {#if plain}{@render children?.()}{:else}
    {#if hero}<div class="hero">{@render hero()}</div>{/if}
    <header>
      {#if leading}<span class="leading">{@render leading()}</span>
      {:else if icon}<span class="glyph"><Icon {icon} size={14} /></span>{/if}
      <span class="titles">
        {#if kind}<span class="kind">{kind}</span>{/if}
        <span class="line">
          <strong class="title" class:one={lines === 1} class:mono {title}>{title}</strong>
          {#if count !== undefined}<span class="count">{count}</span>{/if}
        </span>
      </span>
      {#if aside}<span class="aside">{@render aside()}</span>{/if}
    </header>
    {#if children}<div class="body">{@render children()}</div>{/if}
    {#if footer}<footer>{@render footer()}</footer>{/if}
  {/if}
</article>

<style>
  /* One card on the tonal ladder: the surface rung and a hairline at rest, a
     lit ring when chosen, the float shadow only while carried. */
  .card {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    block-size: 100%;
    min-block-size: 0;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-text);
    overflow: hidden;
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .card:hover,
  .card.active {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .card.selected {
    box-shadow:
      0 0 0 1px var(--color-lit),
      var(--shadow-raised);
  }

  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__node.dragging) .card {
    box-shadow:
      inset 0 0 0 1px var(--color-border),
      var(--shadow-float);
    cursor: grabbing;
  }

  .card.unavailable {
    opacity: 0.6;
  }

  .hero {
    flex: none;
    overflow: hidden;
    background: var(--color-fill);
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
    min-inline-size: 0;
    padding: 12px 12px 8px;
  }

  .dense header {
    padding-block: 8px;
  }

  .row header {
    margin: 4px 4px 0;
    padding: 8px;
    border-radius: var(--radius-row);
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .row:hover header {
    background: var(--row-hover);
  }

  .row.selected header {
    background: var(--row-active);
  }

  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 24px;
    block-size: 24px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-label-secondary);
  }

  .leading {
    display: flex;
    align-items: center;
    flex: none;
  }

  .titles {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
    min-inline-size: 0;
  }

  .kind {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 13px;
  }

  .line {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-inline-size: 0;
  }

  .title {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    min-inline-size: 0;
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
    letter-spacing: -0.005em;
    overflow-wrap: anywhere;
  }

  .title.one {
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }

  .title.mono {
    font-family: var(--font-mono);
    font-size: var(--text-label);
    font-weight: 500;
    letter-spacing: 0;
    word-break: break-all;
  }

  .count,
  .aside {
    flex: none;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
    line-height: 17px;
  }

  .aside {
    align-self: flex-start;
    line-height: 13px;
  }

  .body {
    flex: 1;
    min-block-size: 0;
    padding: 0 12px;
  }

  .body:last-child {
    padding-block-end: 12px;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex: none;
    min-inline-size: 0;
    padding: 4px 12px 12px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 13px;
  }
</style>

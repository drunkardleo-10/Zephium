<script lang="ts">
  import type { Snippet } from "svelte";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "$shared/ui/Icon";
  let {
    kind = "",
    title,
    icon,
    leading,
    hero,
    count,
    lines = 2,
    mono = false,
    selected = false,
    active = false,
    unavailable = false,
    dense = false,
    children,
    footer,
  }: {
    /** Said only where the card does not already say what it is. */
    kind?: string;
    title: string;
    icon?: IconSvgElement;
    leading?: Snippet;
    /** A full-bleed picture above the header. */
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
    children?: Snippet;
    footer?: Snippet;
  } = $props();
</script>

<article class="card" class:selected class:active class:unavailable class:dense>
  {#if hero}<div class="hero work-drag-handle">{@render hero()}</div>{/if}
  <header class="work-drag-handle">
    {#if leading || icon}<span class="glyph">
        {#if leading}{@render leading()}{:else if icon}<Icon {icon} size={14} />{/if}
      </span>{/if}
    <span class="titles">
      {#if kind}<span class="kind">{kind}</span>{/if}
      <span class="line">
        <strong class="title" class:one={lines === 1} class:mono {title}>{title}</strong>
        {#if count !== undefined}<span class="count">{count}</span>{/if}
      </span>
    </span>
  </header>
  {#if children}<div class="body">{@render children()}</div>{/if}
  {#if footer}<footer>{@render footer()}</footer>{/if}
</article>

<style>
  /* One card language: a hairline at rest, a firmer one under the pointer,
     the accent when chosen, and a lift only while the card is carried. */
  .card {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    block-size: 100%;
    min-block-size: 0;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-text);
    overflow: hidden;
    transition: box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .card:hover {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .card.selected {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      0 0 0 2px var(--color-accent-soft),
      0 0 0 3px var(--color-accent);
  }

  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__node.dragging) .card {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      var(--shadow-popover);
  }

  /* The stage the run is working in breathes; it never flashes. */
  .card.active {
    animation: card-active 2.4s var(--ease-smooth) infinite;
  }

  @keyframes card-active {
    0%,
    100% {
      box-shadow: inset 0 0 0 1px var(--color-border);
    }

    50% {
      box-shadow: inset 0 0 0 1px var(--color-accent);
    }
  }

  .card.unavailable {
    opacity: 0.6;
  }

  .hero {
    flex: none;
    overflow: hidden;
    background: var(--color-fill);
    cursor: grab;
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
    padding: 12px 14px 8px;
    cursor: grab;
  }

  .dense header {
    padding: 10px 12px 6px;
  }

  header:active,
  .hero:active {
    cursor: grabbing;
  }

  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 24px;
    block-size: 24px;
    border-radius: var(--radius-xs);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    overflow: hidden;
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
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: var(--text-label);
    font-weight: 500;
    letter-spacing: 0;
    word-break: break-all;
  }

  .count {
    flex: none;
    color: var(--color-faint);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
    line-height: 17px;
  }

  .body {
    flex: 1;
    min-block-size: 0;
    padding: 0 14px;
  }

  .dense .body {
    padding: 0 12px;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex: none;
    min-inline-size: 0;
    padding: 6px 14px 10px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 13px;
  }

  .dense footer {
    padding: 6px 12px 9px;
  }
</style>

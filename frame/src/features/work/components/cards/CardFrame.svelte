<script lang="ts">
  import type { Snippet } from "svelte";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "$shared/ui/Icon";
  let {
    kind,
    title,
    icon,
    leading,
    selected = false,
    unavailable = false,
    dense = false,
    children,
    footer,
  }: {
    kind: string;
    title: string;
    icon?: IconSvgElement;
    leading?: Snippet;
    selected?: boolean;
    unavailable?: boolean;
    dense?: boolean;
    children?: Snippet;
    footer?: Snippet;
  } = $props();
</script>

<article class="card" class:selected class:unavailable class:dense>
  <header class="work-drag-handle">
    <span class="glyph">
      {#if leading}{@render leading()}{:else if icon}<Icon {icon} size={16} />{/if}
    </span>
    <span class="titles">
      <span class="kind">{kind}</span>
      <strong class="title">{title}</strong>
    </span>
  </header>
  {#if children}<div class="body">{@render children()}</div>{/if}
  {#if footer}<footer>{@render footer()}</footer>{/if}
</article>

<style>
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

  .card.unavailable {
    opacity: 0.6;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: none;
    padding: 12px 14px 8px;
    cursor: grab;
  }

  .dense header {
    padding: 10px 12px;
  }

  header:active {
    cursor: grabbing;
  }

  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-sm);
    background: var(--color-fill);
    color: var(--color-label-secondary);
  }

  .titles {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-inline-size: 0;
  }

  .kind {
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 13px;
  }

  .title {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
    letter-spacing: -0.005em;
  }

  .body {
    flex: 1;
    min-block-size: 0;
    padding: 0 14px;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex: none;
    padding: 8px 14px 10px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }
</style>

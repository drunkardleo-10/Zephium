<script lang="ts">
  import type { Snippet } from "svelte";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "$shared/ui/Icon";
  let {
    kind,
    title,
    meta = "",
    icon,
    leading,
    whole = false,
    actions,
  }: {
    kind: string;
    title: string;
    /** One line: host · date · status, whatever the kind has. */
    meta?: string;
    icon?: IconSvgElement;
    leading?: Snippet;
    /** A request reads whole; every other title clamps to two lines. */
    whole?: boolean;
    actions?: Snippet;
  } = $props();
</script>

<header class="lift-header">
  {#if leading || icon}<span class="glyph" aria-hidden="true">
      {#if leading}{@render leading()}{:else if icon}<Icon {icon} size={16} />{/if}
    </span>{/if}
  <div class="titles">
    <p class="kind">{kind}</p>
    <h2 class:whole>{title}</h2>
    {#if meta}<p class="meta">{meta}</p>{/if}
  </div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  .lift-header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding-inline-end: 32px;
  }

  .glyph {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    color: var(--color-muted);
    overflow: hidden;
  }

  .titles {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .kind,
  .meta {
    margin: 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .meta {
    color: var(--color-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  h2 {
    display: -webkit-box;
    margin: 0;
    font-size: var(--text-page-title);
    font-weight: 600;
    line-height: 1.35;
    overflow: hidden;
    overflow-wrap: anywhere;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  h2.whole {
    display: block;
    white-space: pre-wrap;
  }

  .actions {
    display: flex;
    flex: none;
    gap: 6px;
  }
</style>

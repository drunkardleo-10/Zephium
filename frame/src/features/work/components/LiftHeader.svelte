<script lang="ts" module>
  /** The one thing to do with what is lifted: Open page, Save as note, Make tasks. */
  export type LiftAction = {
    label: string;
    onclick?: () => void;
    disabled?: boolean;
    title?: string;
  };
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "$shared/ui/Icon";
  import Button from "$shared/ui/Button";
  import HostGlyph from "./cards/HostGlyph.svelte";
  let {
    kind,
    title,
    meta = "",
    host = "",
    url = "",
    icon,
    leading,
    whole = false,
    onhost,
    primary,
    actions,
  }: {
    kind: string;
    title: string;
    /** One line: date · status, whatever the kind has. */
    meta?: string;
    /** Where it lives: drawn with the site's mark. */
    host?: string;
    url?: string;
    /** The host reads as a quiet link to the thing's own page. */
    onhost?: () => void;
    icon?: IconSvgElement;
    leading?: Snippet;
    /** A request reads whole; every other title clamps to two lines. */
    whole?: boolean;
    primary?: LiftAction;
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
    {#if host || meta}<p class="meta">
        {#if host && onhost}<button
            type="button"
            class="host link"
            title={url || host}
            onclick={onhost}>{host}</button
          >{:else if host}<span class="host"><HostGlyph {host} {url} size={14} />{host}</span>{/if}
        {#if meta}<span class="text">{meta}</span>{/if}
      </p>{/if}
  </div>
  {#if primary || actions}<div class="actions">
      {#if actions}{@render actions()}{/if}
      {#if primary}<Button
          variant="primary"
          size="compact"
          disabled={primary.disabled}
          title={primary.title}
          onclick={primary.onclick}>{primary.label}</Button
        >{/if}
    </div>{/if}
</header>

<style>
  /* Every lift's head: mark, kind, title, where it lives, actions; it stays
     at the top of the well while the body scrolls under it. */
  .lift-header {
    position: sticky;
    inset-block-start: calc(-1 * var(--lift-pad));
    z-index: 3;
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin: calc(-1 * var(--lift-pad)) calc(-1 * var(--lift-pad)) 0;
    padding: var(--lift-pad) calc(var(--lift-pad) + 40px) 12px var(--lift-pad);
    background: var(--color-surface);
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  :global([data-lift-scrolled]) .lift-header {
    box-shadow: 0 1px 0 var(--color-border);
  }

  .glyph {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    overflow: hidden;
  }

  .titles {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 4px;
    min-inline-size: 0;
  }

  .kind,
  .meta {
    margin: 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 13px;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
    color: var(--color-muted);
  }

  .host {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 6px;
    max-inline-size: 60%;
    overflow: hidden;
    white-space: nowrap;
  }

  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    text-decoration: underline;
    text-decoration-color: transparent;
    text-underline-offset: 2px;
    cursor: default;
    transition:
      color var(--motion-fast) var(--ease-out),
      text-decoration-color var(--motion-fast) var(--ease-out);
  }

  .link:hover {
    color: var(--color-text);
    text-decoration-color: currentcolor;
  }

  .link:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
    border-radius: var(--radius-inset);
  }

  .text {
    min-inline-size: 0;
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
    align-items: center;
    gap: 8px;
  }
</style>

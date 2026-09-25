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
    icon?: IconSvgElement;
    leading?: Snippet;
    /** A request reads whole; every other title clamps to two lines. */
    whole?: boolean;
    /** The one thing to do with it: Open page, Save as note, Make tasks. */
    primary?: { label: string; onclick?: () => void; disabled?: boolean; title?: string };
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
        {#if host}<span class="host"><HostGlyph {host} {url} size={14} />{host}</span>{/if}
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
  .lift-header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding-inline-end: 40px;
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

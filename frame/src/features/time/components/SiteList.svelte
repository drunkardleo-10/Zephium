<script lang="ts">
  import { Globe02Icon } from "@hugeicons/core-free-icons";
  import type { SiteTimeView } from "$shared/ipc/bindings";
  import * as m from "$shared/i18n/messages";
  import { favicons } from "$domain/favicons";
  import { duration, share } from "$domain/time";
  import FavIcon from "$shared/ui/FavIcon";

  let {
    sites,
    total,
    limit = Infinity,
    size = "panel",
    selected = null,
    onpoint,
    onselect,
  }: {
    sites: SiteTimeView[];
    /** Seconds the shares are taken of: the period on the web. */
    total: number;
    limit?: number;
    size?: "panel" | "page";
    selected?: string | null;
    /** The site under the pointer, so a chart can bring its part forward. */
    onpoint?: (site: string | null) => void;
    onselect?: (site: string) => void;
  } = $props();

  // Bars measure against the leading site, so the longest fills its row and
  // the rest read against it rather than against a total they never reach.
  let lead = $derived(sites[0]?.seconds ?? 0);
  let shown = $derived(sites.slice(0, limit));
  const percent = new Intl.NumberFormat(undefined, { style: "percent", maximumFractionDigits: 0 });
</script>

<ul class="sites" class:page={size === "page"} aria-label={m.time_sites()}>
  {#if size === "page"}
    <li class="head" aria-hidden="true">
      <span>{m.time_sites()}</span><span>{m.time_opens()}</span><span>{m.time_share()}</span><span
      ></span>
    </li>
  {/if}
  {#each shown as site, index (site.site)}
    {@const mark = favicons.mark(site.icon, `https://${site.site}/`)}
    <li style:--enter={`${Math.min(index, 12) * 22}ms`}>
      <button
        type="button"
        class="row"
        class:selected={selected === site.site}
        aria-pressed={onselect ? selected === site.site : undefined}
        onpointerenter={() => onpoint?.(site.site)}
        onpointerleave={() => onpoint?.(null)}
        onfocus={() => onpoint?.(site.site)}
        onblur={() => onpoint?.(null)}
        onclick={() => onselect?.(site.site)}
      >
        <FavIcon
          image={mark?.image ?? null}
          tone={mark?.tone}
          size={size === "page" ? 18 : 16}
          lit
          fallback={Globe02Icon}
        />
        <span class="name">{site.site}</span>
        {#if size === "page"}
          <span class="number">{site.opens}</span>
          <span class="number">{percent.format(share(site.seconds, total))}</span>
        {/if}
        <span class="spent">{duration(site.seconds)}</span>
        <span class="meter" aria-hidden="true"
          ><span class="fill" style:transform={`scaleX(${share(site.seconds, lead)})`}></span></span
        >
      </button>
    </li>
  {/each}
</ul>

<style>
  .sites {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    animation: enter var(--motion-slow) var(--ease-out) var(--enter) backwards;
  }

  .row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    grid-template-rows: auto auto;
    align-items: center;
    gap: 6px 10px;
    inline-size: 100%;
    padding: 8px 10px 9px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: pointer;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .row:hover {
    background: var(--row-hover);
  }

  .row.selected {
    background: var(--row-active);
    box-shadow: var(--row-rim);
  }

  .row:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .name {
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spent,
  .number {
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
    text-align: end;
    white-space: nowrap;
  }

  .meter {
    grid-column: 2 / -1;
    block-size: 3px;
    overflow: hidden;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
  }

  .fill {
    display: block;
    block-size: 100%;
    border-radius: inherit;
    background: color-mix(in srgb, var(--color-text) 46%, transparent);
    transform-origin: left center;
    transition: transform var(--motion-page) var(--ease-emphasized);
  }

  :global([dir="rtl"]) .fill {
    transform-origin: right center;
  }

  .row:hover .fill,
  .row.selected .fill {
    background: var(--color-text);
  }

  .page .row,
  .head {
    grid-template-columns: auto minmax(0, 1fr) 56px 56px 72px;
  }

  .page .meter {
    grid-column: 2 / -1;
  }

  .head {
    display: grid;
    column-gap: 10px;
    padding: 0 10px 6px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 500;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    animation: none;
  }

  .head span:first-child {
    grid-column: 1 / 3;
  }

  .head span:not(:first-child) {
    text-align: end;
  }

  @keyframes enter {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    li {
      animation: none;
    }

    .fill {
      transition: none;
    }
  }

  :global(:root[data-reduce-motion="true"]) :is(li, .fill) {
    animation: none;
    transition: none;
  }
</style>

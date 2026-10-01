<script lang="ts">
  import { tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import * as m from "$shared/i18n/messages";
  import { tabs } from "$domain/tabs";
  import { reducedMotion } from "$shared/lib/motion";
  import { SITES, type Site } from "../../lib/catalog";
  import { throwMark } from "../../lib/flight";
  import Mark from "../Mark.svelte";

  let {
    kept,
    landing,
    flights,
    onfailed,
  }: {
    /** Kept sites by catalog id, as the projection has them. */
    kept: ReadonlyMap<string, string>;
    /** Where a kept site lands in the window's dock, once it is drawn there. */
    landing: (site: string) => DOMRect | null;
    flights: () => HTMLElement | undefined;
    onfailed: (site: string) => void;
  } = $props();

  const pending = new SvelteSet<string>();

  function source(site: Site) {
    const dark = document.documentElement.dataset.theme !== "light";
    return dark && site.mark.dark ? site.mark.dark : site.mark.light;
  }

  async function toggle(site: Site, plate: HTMLElement) {
    if (pending.has(site.id)) return;
    const existing = kept.get(site.id);
    if (existing) {
      tabs.close(existing);
      return;
    }
    pending.add(site.id);
    const from = plate.querySelector("img")?.getBoundingClientRect() ?? null;
    try {
      const result = await tabs.keepSite(site.id);
      if (result.outcome === "rejected" || result.outcome === "failed") {
        onfailed(site.name);
        return;
      }
      // The projection that draws the new tile arrives with the result.
      await tick();
      const to = landing(site.id);
      const host = flights();
      if (from && to && host && !reducedMotion()) void throwMark(host, source(site), from, to);
    } finally {
      pending.delete(site.id);
    }
  }
</script>

<ul class="apps" aria-label={m.essentials()}>
  {#each SITES as site, index (site.id)}
    {@const on = kept.has(site.id)}
    <li style:--i={index}>
      <button
        type="button"
        class="app"
        aria-pressed={on}
        aria-label={on
          ? m.onb_essentials_remove({ site: site.name })
          : m.onb_essentials_keep({ site: site.name })}
        onclick={(event) => void toggle(site, event.currentTarget)}
      >
        <span class="plate"><Mark mark={site.mark} size={30} /></span>
        <span class="name">{site.name}</span>
      </button>
    </li>
  {/each}
</ul>

<style>
  .apps {
    display: grid;
    grid-template-columns: repeat(4, 64px);
    gap: 30px 22px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    animation: lift 620ms var(--ease-emphasized) backwards;
    animation-delay: calc(240ms + var(--i) * 28ms);
  }

  .app {
    position: relative;
    display: grid;
    justify-items: center;
    gap: 8px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-faint);
    font: inherit;
    font-size: 11.5px;
    font-weight: 500;
    cursor: default;
  }

  .plate {
    display: grid;
    place-items: center;
    inline-size: 64px;
    block-size: 64px;
    border-radius: var(--radius-card);
    background: var(--color-card);
    box-shadow: var(--shadow-raise);
    transition:
      background-color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      scale var(--motion-slow) var(--ease-spring);
  }

  .app:hover .plate {
    background: var(--color-fill-hover);
  }

  .app:active .plate {
    scale: 0.93;
  }

  .app:focus-visible {
    outline: none;
  }

  .app:focus-visible .plate {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .app[aria-pressed="true"] {
    color: var(--color-text);
  }

  .app[aria-pressed="true"] .plate {
    background: var(--color-fill-active);
    box-shadow: inset 0 0 0 1.5px color-mix(in srgb, var(--color-lit) 75%, transparent);
  }

  .name {
    white-space: nowrap;
    transition: color var(--motion-fast) var(--ease-out);
  }

  @keyframes lift {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    li {
      animation: none;
    }
  }
</style>

<script lang="ts">
  import {
    ArrowLeft02Icon,
    ArrowRight02Icon,
    Briefcase02Icon,
    Folder01Icon,
    Globe02Icon,
    PlusSignIcon,
    Refresh01Icon,
    Search01Icon,
    SidebarLeftIcon,
    ToolCaseIcon,
  } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import Icon from "$shared/ui/Icon";
  import { WORDMARK } from "$shared/lib/wordmark";
  import type { Site } from "../lib/catalog";
  import Mark from "./Mark.svelte";
  import WorkCanvas from "./WorkCanvas.svelte";

  let {
    mode,
    sites,
    imported,
    playing,
    element = $bindable(),
  }: {
    mode: "browse" | "work";
    /** What the person has kept, in sidebar order. */
    sites: Site[];
    /** What an import brought, once it has. */
    imported: { from: string; bookmarks: number } | null;
    /** The Work run plays only while it is on screen. */
    playing: boolean;
    element?: HTMLElement;
  } = $props();

  // One row of the dock beside its shelf: the rest are counted, not crammed.
  let shown = $derived(sites.length > 3 ? sites.slice(0, 2) : sites);
  let more = $derived(sites.length > 3 ? sites.length - 2 : 0);
  let now = $state(new Date());
  $effect(() => {
    const timer = setInterval(() => (now = new Date()), 30_000);
    return () => clearInterval(timer);
  });
  let time = $derived(now.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" }));
  let day = $derived(
    now.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" }),
  );
  const number = new Intl.NumberFormat();
</script>

<!-- The browser itself, drawn small from the same tokens it is built from:
     the window onboarding shows is the window it hands over to. -->
<div class="window" data-mode={mode} bind:this={element} aria-hidden="true">
  <aside class="side">
    <div class="browse">
      <div class="top">
        <span class="lights"><i></i><i></i><i></i></span>
        <Icon icon={SidebarLeftIcon} size={14} />
        <span class="gap"></span>
        <Icon icon={ArrowLeft02Icon} size={14} />
        <Icon icon={ArrowRight02Icon} size={14} />
        <Icon icon={Refresh01Icon} size={14} />
      </div>
      <div class="modes">
        <span class="on"><Icon icon={Globe02Icon} size={13} />{m.mode_browse()}</span>
        <span><Icon icon={Briefcase02Icon} size={13} />{m.mode_work()}</span>
      </div>
      <div class="address">{m.search_web_short()}</div>
      <div class="row quiet"><Icon icon={PlusSignIcon} size={14} />{m.new_tab()}</div>
      {#if imported}
        <div class="row folder" data-imported>
          <Icon icon={Folder01Icon} size={14} />{m.onb_window_from({ browser: imported.from })}
        </div>
        <div class="row quiet child">
          {m.onb_window_bookmarks({ count: number.format(imported.bookmarks) })}
        </div>
      {/if}
      <div class="row current"><Icon icon={Globe02Icon} size={14} />{m.new_tab()}</div>
      <div class="dock" data-dock>
        <span class="tile"><Icon icon={ToolCaseIcon} size={15} /></span>
        {#each shown as site (site.id)}
          <span class="tile site" data-kept={site.id}><Mark mark={site.mark} size={17} /></span>
        {/each}
        {#if more}<span class="tile count" data-kept-more>+{more}</span>{/if}
      </div>
    </div>

    <div class="rail">
      <div class="rail-modes">
        <span><Icon icon={Globe02Icon} size={13} /></span>
        <span class="on"><Icon icon={Briefcase02Icon} size={13} /></span>
      </div>
      <span class="rail-icon"><Icon icon={Search01Icon} size={15} /></span>
      <hr />
      {#each sites.slice(0, 4) as site (site.id)}
        <span class="rail-icon"><Mark mark={site.mark} size={17} /></span>
      {/each}
      <span class="rail-icon"><Icon icon={PlusSignIcon} size={15} /></span>
    </div>
  </aside>

  <main class="pane">
    <div class="newtab">
      <div class="search"><Icon icon={Search01Icon} size={13} />{m.search_web_short()}</div>
      <svg class="mark" viewBox="105 90 585 84"
        ><path d={WORDMARK.d} transform="scale({WORDMARK.scaleX} {WORDMARK.scaleY})" /></svg
      >
      <p class="when"><b>{time}</b><span>{day}</span></p>
    </div>
    <div class="work">
      {#if mode === "work"}<WorkCanvas {playing} />{/if}
    </div>
  </main>
</div>

<style>
  .window {
    --side: 232px;

    position: absolute;
    inset: 0;
    display: grid;
    grid-template-columns: var(--side) 1fr;
    gap: 8px;
    padding: 8px;
    box-sizing: border-box;
    overflow: hidden;
    border-radius: var(--radius-card);
    background: var(--color-chrome);
    color: var(--color-text);
    box-shadow:
      var(--shadow-overlay),
      inset 0 0 0 0.5px var(--color-border-strong);
    font-family: var(--font-sans);
    transition: grid-template-columns 820ms var(--ease-emphasized);
  }

  .window[data-mode="work"] {
    --side: 52px;
  }

  .side {
    position: relative;
    min-inline-size: 0;
  }

  .browse,
  .rail {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    transition: opacity 420ms var(--ease-out);
  }

  .browse {
    gap: 6px;
    inline-size: 232px;
  }

  .rail {
    align-items: center;
    gap: 10px;
    inline-size: 52px;
    opacity: 0;
  }

  .window[data-mode="work"] .browse {
    opacity: 0;
  }

  .window[data-mode="work"] .rail {
    opacity: 1;
  }

  .top {
    display: flex;
    align-items: center;
    gap: 10px;
    block-size: 28px;
    padding-inline: 4px;
    color: var(--color-faint);
  }

  .lights {
    display: flex;
    gap: 6px;
    margin-inline-end: 4px;
  }

  .lights i {
    inline-size: 10px;
    block-size: 10px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-strong);
  }

  .gap {
    flex: 1;
  }

  .modes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    block-size: 30px;
    padding: 2px;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    font-size: 12px;
    font-weight: 500;
  }

  .modes span {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border-radius: var(--radius-inset);
    color: var(--color-faint);
  }

  .modes .on {
    background: var(--color-raise-active);
    box-shadow: var(--shadow-raise);
    color: var(--color-text);
  }

  .address {
    display: grid;
    place-items: center;
    block-size: 30px;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    color: var(--color-muted);
    font-size: 12px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 9px;
    block-size: 30px;
    padding-inline: 9px;
    border-radius: var(--radius-inset);
    font-size: 12.5px;
    font-weight: var(--sidebar-row-weight);
    white-space: nowrap;
    color: var(--color-label-secondary);
  }

  .row :global(svg) {
    flex: none;
    color: var(--color-faint);
  }

  .row.quiet {
    color: var(--color-faint);
  }

  .row.current {
    margin-block-start: 4px;
    background: var(--row-active);
    box-shadow: var(--row-rim);
    color: var(--color-text);
  }

  .row.folder {
    animation: row-in 600ms var(--ease-emphasized) backwards;
  }

  .row.child {
    padding-inline-start: 32px;
    animation: row-in 600ms var(--ease-emphasized) 120ms backwards;
  }

  .dock {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 5px;
    margin-block-start: auto;
  }

  .tile {
    display: grid;
    place-items: center;
    block-size: 40px;
    border-radius: var(--radius-row);
    background: var(--color-card);
    color: var(--color-muted);
    font-size: 11.5px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .tile.site,
  .tile.count {
    animation: tile-in 520ms var(--ease-snap) backwards;
  }

  .rail-modes {
    display: grid;
    gap: 2px;
    margin-block-start: 30px;
    padding: 3px;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
  }

  .rail-modes span {
    display: grid;
    place-items: center;
    inline-size: 30px;
    block-size: 24px;
    border-radius: var(--radius-inset);
    color: var(--color-faint);
  }

  .rail-modes .on {
    background: var(--color-raise-active);
    color: var(--color-text);
  }

  .rail-icon {
    display: grid;
    place-items: center;
    inline-size: 36px;
    block-size: 36px;
    border-radius: var(--radius-control-compact);
    color: var(--color-muted);
  }

  .rail hr {
    inline-size: 22px;
    margin: 2px 0;
    border: 0;
    border-block-start: 1px solid var(--color-border);
  }

  .pane {
    position: relative;
    overflow: hidden;
    border-radius: var(--content-radius);
    background: var(--color-page);
    box-shadow: inset 0 0 0 0.5px var(--color-border);
  }

  .newtab,
  .work {
    position: absolute;
    inset: 0;
    transition: opacity 520ms var(--ease-out);
  }

  .work {
    opacity: 0;
  }

  .window[data-mode="work"] .newtab {
    opacity: 0;
  }

  .window[data-mode="work"] .work {
    opacity: 1;
  }

  .search {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 50%;
    display: flex;
    align-items: center;
    gap: 9px;
    inline-size: 380px;
    block-size: 38px;
    padding-inline: 16px;
    box-sizing: border-box;
    transform: translateX(-50%);
    border-radius: 0 0 var(--radius-control) var(--radius-control);
    background: var(--color-chrome);
    color: var(--color-faint);
    font-size: 12.5px;
  }

  .mark {
    position: absolute;
    inset-inline-start: 162px;
    inset-block-start: 236px;
    inline-size: 300px;
    block-size: 43px;
    overflow: visible;
  }

  .mark path {
    fill: var(--color-text);
  }

  .when {
    position: absolute;
    inset-block-start: 304px;
    inset-inline: 0;
    display: flex;
    justify-content: center;
    gap: 10px;
    margin: 0;
    color: var(--color-muted);
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }

  .when b {
    color: var(--color-text);
    font-weight: 500;
  }

  @keyframes row-in {
    from {
      opacity: 0;
      transform: translateX(-10px);
    }
  }

  @keyframes tile-in {
    from {
      opacity: 0;
      transform: scale(0.6);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .window,
    .browse,
    .rail,
    .newtab,
    .work {
      transition: none;
    }

    .row.folder,
    .row.child,
    .tile.site,
    .tile.count {
      animation: none;
    }
  }
</style>

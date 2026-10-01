<script lang="ts">
  import {
    CheckListIcon,
    Download01Icon,
    HistoryIcon,
    Note01Icon,
    Search01Icon,
  } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import Icon from "$shared/ui/Icon";
  import { SITES } from "../lib/catalog";
  import Mark from "./Mark.svelte";

  let {
    typing,
    away,
  }: {
    /** Types its search out once it has opened. */
    typing: boolean;
    /** Steps aside while the real launcher is up, so there is only ever one. */
    away: boolean;
  } = $props();

  const QUERY = "fig";
  const figma = SITES.find((site) => site.id === "figma")!.mark;
  const destinations = [
    { label: m.tool_tasks, icon: CheckListIcon },
    { label: m.tool_notes, icon: Note01Icon },
    { label: m.browser_history_title, icon: HistoryIcon },
    { label: m.browser_downloads_title, icon: Download01Icon },
  ];

  // Typed out the way a person would: a few letters, then what they reach.
  let typed = $state("");
  $effect(() => {
    if (!typing) {
      typed = "";
      return;
    }
    const timers = QUERY.split("").map((_, index) =>
      setTimeout(() => (typed = QUERY.slice(0, index + 1)), 380 + index * 140),
    );
    return () => timers.forEach(clearTimeout);
  });
  let open = $derived(typed.length === QUERY.length);
</script>

<!-- The launcher as it is drawn: the field as its own capsule of glass, and
     the results as a sheet under it that opens once there is something to
     show. Only the launcher has the stage here; the browser is not needed
     to find anything. -->
<div class="launcher" data-open={open} data-away={away} aria-hidden="true">
  <div class="capsule">
    <Icon icon={Search01Icon} size={20} />
    <span class="field"
      >{#if typed}<span class="typed">{typed}</span>{:else}<span class="placeholder"
          >{m.panel_search_placeholder()}</span
        >{/if}<i class="caret"></i></span
    >
  </div>
  <div class="sheet">
    <div class="rows">
      <div class="row selected">
        <Mark mark={figma} size={16} /><span class="title"><mark>Fig</mark>ma · Design system</span
        ><span class="detail">{m.onb_launcher_switch()}</span><kbd>↵</kbd>
      </div>
      <div class="row">
        <Mark mark={figma} size={16} /><span class="title"><mark>fig</mark>ma.com/files</span><span
          class="detail">{m.onb_launcher_history()}</span
        >
      </div>
      <div class="row">
        <Icon icon={Note01Icon} size={16} /><span class="title"
          ><mark>Fig</mark>ma handoff notes</span
        ><span class="detail">{m.tool_notes()}</span>
      </div>
      <div class="row">
        <Icon icon={Search01Icon} size={16} /><span class="title"
          >{m.onb_launcher_web({ query: QUERY })}</span
        >
      </div>
    </div>
    <footer>
      {#each destinations as destination, index (index)}
        <span class="chip"><Icon icon={destination.icon} size={14} />{destination.label()}</span>
      {/each}
    </footer>
  </div>
</div>

<style>
  .launcher {
    display: grid;
    gap: 8px;
    inline-size: 640px;
    transition:
      opacity 320ms var(--ease-out),
      transform 420ms var(--ease-emphasized);
  }

  .launcher[data-away="true"] {
    opacity: 0;
    transform: translateY(10px) scale(0.97);
  }

  .capsule,
  .sheet {
    background: color-mix(in srgb, var(--color-chrome) var(--wash-launcher), transparent);
    box-shadow:
      var(--shadow-overlay),
      inset 0 0 0 1px var(--color-border-strong);
  }

  .capsule {
    display: flex;
    align-items: center;
    gap: 14px;
    block-size: 60px;
    padding: 0 24px 0 22px;
    border-radius: var(--radius-capsule);
    color: var(--color-muted);
  }

  .field {
    display: flex;
    align-items: center;
    flex: 1;
    font-size: 21px;
    letter-spacing: -0.016em;
  }

  .typed {
    color: var(--color-text);
  }

  .placeholder {
    color: var(--color-faint);
  }

  .caret {
    inline-size: 1.5px;
    block-size: 22px;
    margin-inline-start: 1px;
    background: var(--color-text);
    animation: blink 1.1s steps(2) infinite;
  }

  .sheet {
    display: grid;
    overflow: hidden;
    border-radius: var(--radius-panel);
    transform-origin: top center;
    opacity: 0;
    transform: translateY(-6px) scale(0.985);
    transition:
      opacity 260ms var(--ease-smooth),
      transform 260ms var(--ease-smooth);
  }

  .launcher[data-open="true"] .sheet {
    opacity: 1;
    transform: none;
  }

  .rows {
    display: grid;
    gap: 2px;
    padding: 6px 6px 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    block-size: 36px;
    padding: 0 10px;
    border-radius: var(--radius-card);
    color: var(--color-muted);
  }

  .row.selected {
    background: var(--color-fill-active);
    color: var(--color-text);
  }

  .title {
    flex: 1;
    min-inline-size: 0;
    overflow: hidden;
    font-size: 13.5px;
    line-height: 20px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  mark {
    background: none;
    color: var(--color-text);
    font-weight: 550;
  }

  .detail {
    color: var(--color-faint);
    font-size: 11.5px;
  }

  kbd {
    display: inline-grid;
    place-items: center;
    min-inline-size: 18px;
    block-size: 18px;
    border-radius: 5px;
    background: var(--color-fill);
    color: var(--color-faint);
    font-family: var(--font-sans);
    font-size: 11px;
  }

  footer {
    display: flex;
    gap: 4px;
    block-size: 52px;
    align-items: center;
    padding: 0 10px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    block-size: 30px;
    padding: 0 10px 0 9px;
    border-radius: var(--radius-capsule);
    color: var(--color-muted);
    font-size: 12.5px;
  }

  @keyframes blink {
    50% {
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .caret {
      animation: none;
    }

    .launcher,
    .sheet {
      transition: none;
    }
  }
</style>

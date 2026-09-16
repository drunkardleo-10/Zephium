<script lang="ts">
  import { Delete02Icon, Globe02Icon } from "@hugeicons/core-free-icons";
  import FavIcon from "$shared/ui/FavIcon";
  import IconButton from "$shared/ui/IconButton";
  import { favicons } from "$domain/favicons";
  import * as m from "$shared/i18n/messages";
  import { hostOf, matchRange, timeLabel, toRows, visitedAt } from "../lib/history-model";
  import { offsetsOf, windowFor, WINDOW_THRESHOLD } from "../lib/history-window";
  import type { HistorySession } from "../lib/history-session.svelte";

  let {
    session,
    density = "page",
    onopen,
  }: {
    session: HistorySession;
    density?: "page" | "panel";
    onopen: (url: string, newTab: boolean) => void;
  } = $props();

  const heights = $derived(density === "page" ? { day: 44, visit: 46 } : { day: 32, visit: 34 });

  let scroller: HTMLElement | undefined = $state();
  let scrollTop = $state(0);
  let viewport = $state(0);
  let selected = $state<string | null>(null);

  let today = $state(new Date());
  let rows = $derived(
    toRows(session.visits, today, { today: m.history_today(), yesterday: m.history_yesterday() }),
  );
  let windowed = $derived(rows.length > WINDOW_THRESHOLD);
  let offsets = $derived(
    windowed
      ? offsetsOf(
          rows.map((row) => row.kind),
          heights,
        )
      : [],
  );
  let slice = $derived(
    windowed
      ? windowFor(offsets, scrollTop, viewport)
      : { first: 0, last: rows.length, before: 0, after: 0 },
  );
  let visible = $derived(rows.slice(slice.first, slice.last));

  // A day boundary while the page is open must not leave "Today" on yesterday.
  $effect(() => {
    const timer = setInterval(() => {
      const now = new Date();
      if (now.getDate() !== today.getDate()) today = now;
    }, 60_000);
    return () => clearInterval(timer);
  });

  function measure(element: HTMLElement) {
    viewport = element.clientHeight;
    const observer = new ResizeObserver(() => {
      viewport = element.clientHeight;
    });
    observer.observe(element);
    return { destroy: () => observer.disconnect() };
  }

  function onscroll(event: Event & { currentTarget: HTMLElement }) {
    scrollTop = event.currentTarget.scrollTop;
    const { scrollHeight, clientHeight } = event.currentTarget;
    if (scrollTop + clientHeight >= scrollHeight - clientHeight) void session.reload(true);
  }

  function move(offset: number) {
    const visits = rows.filter((row) => row.kind === "visit");
    if (!visits.length) return;
    const at = visits.findIndex((row) => row.id === selected);
    const next = Math.min(visits.length - 1, Math.max(0, at < 0 ? 0 : at + offset));
    selected = visits[next]?.id ?? null;
    scroller?.querySelector<HTMLElement>(`[data-row="${selected}"]`)?.scrollIntoView({
      block: "nearest",
    });
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      move(1);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      move(-1);
      return;
    }
    const row = rows.find((candidate) => candidate.id === selected);
    if (!row || row.kind !== "visit") return;
    if (event.key === "Enter") {
      event.preventDefault();
      onopen(row.visit.url, event.metaKey || event.ctrlKey);
    } else if (event.key === "Backspace" || event.key === "Delete") {
      event.preventDefault();
      void session.forget([row.visit.url]);
    }
  }
</script>

<div
  class="scroller"
  data-density={density}
  bind:this={scroller}
  use:measure
  {onscroll}
  onkeydown={keydown}
  role="listbox"
  aria-label={m.browser_history_title()}
  aria-activedescendant={selected ?? undefined}
  tabindex="0"
>
  <div class="spacer" style:block-size={`${slice.before}px`}></div>
  {#each visible as row (row.id)}
    {#if row.kind === "day"}
      <h2 class="day" style:block-size={`${heights.day}px`}>
        {row.label}<span class="count">{row.count}</span>
      </h2>
    {:else}
      {@const at = visitedAt(row.visit)}
      {@const range = matchRange(row.visit.title, session.query)}
      <div
        class="visit"
        class:selected={selected === row.id}
        data-row={row.id}
        id={row.id}
        style:block-size={`${heights.visit}px`}
        role="option"
        aria-selected={selected === row.id}
        tabindex="-1"
        onpointermove={() => (selected = row.id)}
        onclick={(event) => onopen(row.visit.url, event.metaKey || event.ctrlKey)}
        onkeydown={(event) => {
          if (event.key === " ") {
            event.preventDefault();
            onopen(row.visit.url, false);
          }
        }}
      >
        <FavIcon image={favicons.image(row.visit.icon)} size={16} lit fallback={Globe02Icon} />
        <span class="title"
          >{#if range}{row.visit.title.slice(0, range[0])}<mark
              >{row.visit.title.slice(range[0], range[1])}</mark
            >{row.visit.title.slice(range[1])}{:else}{row.visit.title}{/if}</span
        >
        <span class="host">{hostOf(row.visit.url)}</span>
        <time datetime={at.toISOString()}>{timeLabel(at)}</time>
        <span class="forget"
          ><IconButton
            icon={Delete02Icon}
            label={m.history_forget()}
            size={14}
            buttonSize={22}
            onclick={(event) => {
              event.stopPropagation();
              void session.forget([row.visit.url]);
            }}
          /></span
        >
      </div>
    {/if}
  {/each}
  <div class="spacer" style:block-size={`${slice.after}px`}></div>
  {#if session.capped}
    <p class="notice">{m.history_capped()}</p>
  {:else if !session.exhausted}
    <button type="button" class="more" onclick={() => void session.reload(true)}
      >{m.history_more()}</button
    >
  {/if}
</div>

<style>
  .scroller {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    outline: none;
    scrollbar-width: thin;
  }

  .scroller[data-density="panel"] {
    padding: 8px 8px 12px;
  }

  .day {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    margin: 0;
    padding: 0 10px 6px;
    font-size: 12px;
    font-weight: 550;
    letter-spacing: 0.01em;
    color: var(--color-muted);
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--color-page);
  }

  /* The sidebar tool does not sit on the page ground, so a sticky heading
     there would paint the wrong colour over the rows behind it. */
  .scroller[data-density="panel"] .day {
    position: static;
    background: none;
  }

  .count {
    font-weight: 400;
    color: var(--color-faint);
  }

  .visit {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
    border-radius: 9px;
    cursor: default;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  .visit.selected {
    background: var(--color-fill-active);
  }

  :global(:root[data-theme="light"]) .visit.selected {
    background: var(--color-fill-pressed);
  }

  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 13.5px;
    color: var(--color-text);
  }

  /* The part the typing accounts for carries the weight; the rest recedes. */
  mark {
    background: none;
    color: var(--color-text);
    font-weight: 550;
  }

  .host {
    flex: none;
    max-width: 34%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 11.5px;
    color: var(--color-faint);
  }

  time {
    flex: none;
    width: 62px;
    text-align: end;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    color: var(--color-faint);
  }

  /* Reserved so revealing it never shifts the row beneath the pointer. */
  .forget {
    flex: none;
    visibility: hidden;
  }

  .forget:focus-within {
    visibility: visible;
  }

  .visit.selected .forget {
    visibility: visible;
  }

  .spacer {
    flex: none;
  }

  .notice,
  .more {
    display: block;
    width: 100%;
    margin: 12px 0 0;
    padding: 10px;
    border: 0;
    background: none;
    font-size: 12px;
    color: var(--color-muted);
    text-align: center;
  }

  .more {
    cursor: default;
    border-radius: 9px;
  }

  .more:hover {
    background: var(--color-fill-hover);
  }

  .scroller[data-density="panel"] .title {
    font-size: 12.5px;
  }

  .scroller[data-density="panel"] .host,
  .scroller[data-density="panel"] time {
    display: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .visit {
      transition: none;
    }
  }

  @media (forced-colors: active) {
    .visit.selected {
      outline: 1px solid Highlight;
    }
  }
</style>

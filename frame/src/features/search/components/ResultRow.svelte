<script lang="ts">
  import {
    Search01Icon,
    Globe02Icon,
    CommandLineIcon,
    Clock01Icon,
    Note01Icon,
  } from "@hugeicons/core-free-icons";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import type { SearchResult } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import FavIcon from "$shared/ui/FavIcon";
  import { favicons } from "$domain/favicons";
  import { IS_MAC } from "$shared/platform";
  import { matchRange, resultDetail } from "../lib/search-model";

  let {
    result = null,
    title,
    icon = null,
    id,
    index,
    setsize,
    selected,
    query,
    onhover,
    onrun,
  }: {
    result?: SearchResult | null;
    title: string;
    /** Set for a tool destination, which has no native result behind it. */
    icon?: IconSvgElement | null;
    id: string;
    index: number;
    setsize: number;
    selected: boolean;
    query: string;
    onhover: () => void;
    onrun: () => void;
  } = $props();

  let detail = $derived(result ? resultDetail(result) : "");
  let range = $derived(matchRange(title, query));
  // A site the user has reached is shown by its own mark. The clock belongs to
  // a recorded search, not to every visited page.
  let sited = $derived(
    !!result && (result.kind === "tab" || result.kind === "history" || result.kind === "url"),
  );
  let image = $derived(sited ? favicons.image(result?.icon) : null);
  let glyph = $derived.by(() => {
    if (icon) return icon;
    switch (result?.kind) {
      case "search":
      case "suggestion":
        return Search01Icon;
      case "search_history":
        return Clock01Icon;
      case "note":
        return Note01Icon;
      case "command":
        return CommandLineIcon;
      default:
        return Globe02Icon;
    }
  });
  let shortcut = $derived(
    result?.kind === "command" && result.detail
      ? result.detail
          .replace("CmdOrCtrl+", IS_MAC ? "⌘" : "Ctrl+")
          .replace("Shift+", IS_MAC ? "⇧" : "Shift+")
      : "",
  );
</script>

<button
  type="button"
  {id}
  class="row"
  class:selected
  tabindex="-1"
  role="option"
  aria-selected={selected}
  aria-posinset={index + 1}
  aria-setsize={setsize}
  onpointermove={onhover}
  onpointerdown={(event) => {
    // Keep the field focused so the list does not dismiss itself out from
    // under the pointer before the click lands.
    if (event.pointerType !== "touch") event.preventDefault();
  }}
  onclick={onrun}
>
  <span class="glyph">
    {#if image}<FavIcon
        {image}
        tone={favicons.tone(result?.icon)}
        size={16}
        lit
        fallback={Globe02Icon}
      />
    {:else}<Icon icon={glyph} size={16} />{/if}
  </span>
  <span class="title"
    >{#if range}{title.slice(0, range[0])}<mark>{title.slice(range[0], range[1])}</mark
      >{title.slice(range[1])}{:else}{title}{/if}</span
  >
  {#if detail}<span class="detail">{detail}</span>{/if}
  {#if shortcut}<kbd>{shortcut}</kbd>{/if}
</button>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  /* Deliberately no per-row entry animation. The typed row's action carries the
     query, so its identity changes on every keystroke and it would re-fade as
     fast as the user types — which reads as exactly the lag it was meant to
     soften. The list settles by resizing instead. */

  /* One filled shape, no ring. An inset hairline here reads as a text field
     nested inside the list rather than as a selected row.

     Light chrome normally elevates by getting brighter, but that idiom assumes
     a grey ground: a white pill on a white result surface is invisible. Over
     this surface the selection is a light-touch overlay instead. */
  .row.selected {
    background: var(--color-fill-active);
  }

  :global(:root[data-theme="light"]) .row.selected {
    background: var(--color-fill-pressed);
  }

  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    width: 18px;
    height: 18px;
    color: var(--color-muted);
  }

  .row.selected .glyph {
    color: var(--color-text);
  }

  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 13.5px;
    font-weight: 400;
    line-height: 20px;
    color: var(--color-muted);
  }

  .row.selected .title {
    color: var(--color-text);
  }

  /* The part the typing accounts for carries the weight; the rest recedes. */
  mark {
    background: none;
    color: var(--color-text);
    font-weight: 550;
  }

  .detail {
    flex: none;
    max-width: 38%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 11.5px;
    color: var(--color-faint);
  }

  kbd {
    flex: none;
    max-width: 130px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-family: var(--font-sans);
    font-size: 11px;
    color: var(--color-faint);
  }

  @media (prefers-reduced-motion: reduce) {
    .row {
      transition: none;
    }
  }

  @media (forced-colors: active) {
    .row.selected {
      outline: 1px solid Highlight;
    }
  }
</style>

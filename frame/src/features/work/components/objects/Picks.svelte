<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { ObjectActions, PicksView, PickView } from "../../lib/board/types";
  import PickCard from "./PickCard.svelte";
  import FlightCard from "./FlightCard.svelte";
  import Title from "./Title.svelte";
  import Sheet from "./Sheet.svelte";
  import { picksSheet } from "./centre";
  import PageFace from "../run/PageFace.svelte";
  /** A card's least width, pictured or plain, and the gap between cards. */
  const CARD = { pictured: 200, plain: 220, gap: 16 } as const;
  /** Things to choose between, side by side; the set opens as a sheet to compare. */
  let {
    object,
    actions = {},
    centre = false,
  }: {
    object: PicksView;
    actions?: ObjectActions;
    /** Opened in the centre: all of it, at reading size. */
    centre?: boolean;
  } = $props();
  let width = $state(0);
  /**
   * Cards to a row: as many as the width holds at a card's least, then spread
   * over balanced rows, so five stand 3 + 2 (or 5 across), never 4 + 1.
   */
  const across = $derived.by(() => {
    const count = object.items.length;
    if (!count) return 1;
    const least = pictured ? CARD.pictured : CARD.plain;
    const fits = width
      ? Math.max(1, Math.floor((width + CARD.gap) / (least + CARD.gap)))
      : Math.min(count, 4);
    const rows = Math.ceil(count / Math.min(fits, count));
    return Math.ceil(count / rows);
  });
  const routed = (pick: PickView): pick is PickView & { route: NonNullable<PickView["route"]> } =>
    !!pick.route;
  const flights = $derived(object.facet === "flight" && object.items.every(routed));
  const pictured = $derived(object.items.some((item) => item.picture));
  /** Each thing stands under the page it was taken from, when the set came from several. */
  const sourced = $derived(!flights && !centre && object.items.some((item) => item.from));
  /** The column looked at: its page and its thing light up together. */
  let lit = $state<number | null>(null);
  /** What a card holds under its name: a set shares one height only when its cards hold alike. */
  const holds = (pick: PickView) =>
    pick.facts.length || pick.price || pick.why ? `${pick.facts.length}|${!!pick.price}` : "";
  /** Only a set of pictured, alike cards shares one height; the rest keep their own. */
  const mixed = $derived(
    !object.items.every((item) => item.picture) ||
      object.items.some((item) => !holds(item)) ||
      new Set(object.items.map(holds)).size > 1,
  );
</script>

<section
  class="picks"
  class:flights
  class:pictured
  class:mixed
  aria-label={object.title}
  bind:clientWidth={width}
  style:--across={across}
>
  {#if object.title || (actions.compare && object.items.length > 1)}
    <header>
      {#if object.title}<Title text={object.title} />{/if}
      {#if actions.compare && object.items.length > 1}<button
          type="button"
          class="compare nodrag nopan"
          onclick={() => actions.compare?.(object.id)}>{m.work_pick_compare()}</button
        >{/if}
    </header>
  {/if}
  <div class="set" class:sourced>
    {#each object.items as pick, index (index)}
      {#if flights && routed(pick)}<FlightCard {pick} {actions} />
      {:else if sourced}<div
          class="cell"
          class:lit={lit === index}
          role="group"
          onpointerenter={() => (lit = index)}
          onpointerleave={() => lit === index && (lit = null)}
        >
          {#if pick.from}{@const from = pick.from}<button
              type="button"
              class="from nodrag nopan"
              aria-label={from.title || from.url}
              onclick={(event) => {
                event.stopPropagation();
                actions.link?.(from.url);
              }}><PageFace url={from.url} title={from.title} frame={from.frame} /></button
            ><span class="stem" aria-hidden="true"></span>{:else}<span
              class="from none"
              aria-hidden="true"
            ></span><span class="stem none" aria-hidden="true"></span>{/if}
          <PickCard
            {pick}
            {actions}
            video={object.facet === "video"}
            onopen={actions.open ? () => actions.open?.(object.id, index) : undefined}
          />
        </div>
      {:else}<PickCard
          {pick}
          {actions}
          video={object.facet === "video"}
          onopen={actions.open ? () => actions.open?.(object.id, index) : undefined}
        />{/if}
    {/each}
  </div>
  {#if centre && object.items.length > 1 && !flights}<div class="compare-sheet">
      <Sheet object={picksSheet(object)} {actions} rows={object.items.length} />
    </div>{/if}
</section>

<style>
  .picks {
    display: flex;
    flex-direction: column;
    gap: 14px;
    inline-size: 100%;
  }

  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 16px;
    padding-inline: 2px;
  }

  .compare {
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    opacity: 0;
    cursor: default;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .compare:hover {
    color: var(--color-text);
  }

  .compare:focus-visible,
  .picks:hover .compare {
    opacity: 1;
  }

  .set {
    display: grid;
    grid-template-columns: repeat(var(--across), minmax(0, 1fr));
    align-items: stretch;
    gap: 16px;
  }

  .mixed .set {
    align-items: start;
  }

  /* A page over what was taken from it: the window, a short stem, the thing. */
  .set.sourced {
    position: relative;
    align-items: start;
  }

  /* One quiet trunk through the pages' bars: they were read in one go. */
  .set.sourced::before {
    position: absolute;
    inset-block-start: 14px;
    inset-inline: 0;
    border-block-start: 1px solid var(--color-border);
    content: "";
  }

  .cell {
    position: relative;
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }

  .from {
    position: relative;
    display: block;
    inline-size: 100%;
    aspect-ratio: 16 / 11;
    padding: 0;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .from.none {
    visibility: hidden;
  }

  .from:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .stem {
    align-self: center;
    inline-size: 1px;
    block-size: 18px;
    background: var(--color-border-strong);
    transition: background var(--motion-fast) var(--ease-out);
  }

  .stem.none {
    visibility: hidden;
  }

  .cell.lit .stem {
    background: var(--color-muted);
  }

  .set.sourced:has(.cell.lit) .cell:not(.lit) .from {
    opacity: 0.55;
  }

  .flights .set {
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }

  .compare-sheet {
    margin-block-start: 16px;
  }
</style>

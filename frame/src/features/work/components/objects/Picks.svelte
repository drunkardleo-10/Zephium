<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { ObjectActions, PicksView, PickView } from "../../lib/board/types";
  import PickCard from "./PickCard.svelte";
  import FlightCard from "./FlightCard.svelte";
  import Title from "./Title.svelte";
  import Sheet from "./Sheet.svelte";
  import { picksSheet } from "./centre";
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
  const routed = (pick: PickView): pick is PickView & { route: NonNullable<PickView["route"]> } =>
    !!pick.route;
  const flights = $derived(object.facet === "flight" && object.items.every(routed));
  const pictured = $derived(object.items.some((item) => item.picture));
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
  style:--across={Math.min(object.items.length, flights ? 3 : 4)}
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
  <div class="set">
    {#each object.items as pick, index (index)}
      {#if flights && routed(pick)}<FlightCard {pick} {actions} />
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

  .flights .set {
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }

  .compare-sheet {
    margin-block-start: 16px;
  }
</style>

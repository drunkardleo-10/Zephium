<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { Detail, ObjectActions, PicksView, PickView } from "../../lib/board/types";
  import PickCard from "./PickCard.svelte";
  import FlightCard from "./FlightCard.svelte";
  import Title from "./Title.svelte";
  import Sheet from "./Sheet.svelte";
  import { picksSheet } from "./centre";
  /** Things to choose between, side by side; the set opens as a sheet to compare. */
  let {
    object,
    detail,
    actions = {},
    centre = false,
  }: {
    object: PicksView;
    detail: Detail;
    actions?: ObjectActions;
    /** Opened in the centre: all of it, at reading size. */
    centre?: boolean;
  } = $props();
  const routed = (pick: PickView): pick is PickView & { route: NonNullable<PickView["route"]> } =>
    !!pick.route;
  const flights = $derived(object.facet === "flight" && object.items.every(routed));
  const pictured = $derived(object.items.some((item) => item.picture));
  /** Some with pictures and some without: each card keeps its own height. */
  const mixed = $derived(pictured && object.items.some((item) => !item.picture));
  /** At a distance a set shows its first few; the rest wait for a closer look. */
  const shown = $derived(
    detail !== "tile" || flights
      ? object.items
      : object.items.some((item) => item.picture)
        ? object.items.filter((item) => item.picture).slice(0, 4)
        : [],
  );
</script>

<section
  class="picks {detail}"
  class:flights
  class:pictured
  class:mixed
  aria-label={object.title}
  style:--across={Math.min(object.items.length, flights ? 3 : 4)}
>
  {#if object.title || (actions.compare && object.items.length > 1)}
    <header>
      {#if object.title}<Title text={object.title} {detail} level={3} />{/if}
      {#if detail === "full" && actions.compare && object.items.length > 1}<button
          type="button"
          class="compare nodrag nopan"
          onclick={() => actions.compare?.(object.id)}>{m.work_pick_compare()}</button
        >{/if}
    </header>
  {/if}
  <div class="set">
    {#each shown as pick, index (index)}
      {#if flights && routed(pick)}<FlightCard {pick} {detail} {actions} />
      {:else}<PickCard
          {pick}
          {detail}
          {actions}
          video={object.facet === "video"}
          onopen={actions.open ? () => actions.open?.(object.id, index) : undefined}
        />{/if}
    {/each}
  </div>
  {#if centre && object.items.length > 1 && !flights}<div class="compare-sheet">
      <Sheet object={picksSheet(object)} detail="full" {actions} rows={object.items.length} />
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

  .overview {
    gap: 22px;
  }

  .overview .set {
    gap: 24px;
  }

  .tile .set {
    gap: 24px;
  }
</style>

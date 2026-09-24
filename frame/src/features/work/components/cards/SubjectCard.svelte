<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import SubjectPicture from "./SubjectPicture.svelte";
  import { displayHost } from "$shared/ui/data/Artifact";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const host = $derived(displayHost(item.subject?.homepage));
  // The card is sized for a picture as soon as the run found one to admit:
  // until one is, the tile stands quietly in its place.
  const pictured = $derived(!!item.image || !!item.subject?.imageCandidates?.length);
  /** The lead fact (the price, when there is one) and one more; the lift has the rest. */
  const facts = $derived((item.facts ?? []).slice(0, 2));
</script>

{#snippet picture()}<div class="picture">
    <SubjectPicture picture={item.image} name={item.title} large />
  </div>{/snippet}
{#snippet tile()}<SubjectPicture picture={item.image} name={item.title} />{/snippet}

<CardFrame
  title={item.title}
  {selected}
  unavailable={item.unavailable}
  hero={pictured ? picture : undefined}
  leading={pictured ? undefined : tile}
  dense
>
  {#if facts.length}
    <dl class="facts">
      {#each facts as fact, index (fact.label)}
        <div class="fact" class:lead={index === 0}>
          <dt>{fact.label}</dt>
          <dd title={fact.value}>{fact.value}</dd>
        </div>
      {/each}
    </dl>
  {:else if item.detail}<p class="descriptor">{item.detail}</p>{/if}
  {#snippet footer()}<span class="host">{host || item.status}</span>{/snippet}
</CardFrame>

<style>
  .picture {
    block-size: 112px;
  }

  .facts {
    display: grid;
    gap: 2px;
    margin: 0;
  }

  .fact {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    min-inline-size: 0;
    font-size: var(--text-label);
    line-height: 16px;
  }

  .fact dt {
    flex: none;
    max-inline-size: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-muted);
    text-transform: capitalize;
  }

  .fact dd {
    min-inline-size: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .fact.lead dd {
    font-weight: 600;
  }

  .descriptor {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .host {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>

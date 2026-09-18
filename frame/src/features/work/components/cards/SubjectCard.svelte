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
</script>

<CardFrame kind={item.kind} title={item.title} {selected} unavailable={item.unavailable} dense>
  {#snippet leading()}<span class="picture">
      <SubjectPicture picture={item.image} name={item.title} />
    </span>{/snippet}
  {#if pictured}<div class="hero">
      <SubjectPicture picture={item.image} name={item.title} large />
    </div>{/if}
  {#if item.facts?.length}
    <dl class="facts">
      {#each item.facts as fact, index (fact.label)}
        <div class="fact" class:lead={index === 0}>
          <dt>{fact.label}</dt>
          <dd>{fact.value}</dd>
        </div>
      {/each}
    </dl>
  {:else if item.detail}<p class="descriptor">{item.detail}</p>{/if}
  {#snippet footer()}<span>{host || item.status}</span>{/snippet}
</CardFrame>

<style>
  .picture {
    display: block;
    flex: none;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .hero {
    block-size: 120px;
    margin-block-end: 6px;
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .facts {
    display: grid;
    gap: 2px;
    margin: 0;
  }

  .fact {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: var(--text-label);
    line-height: 16px;
  }

  .fact dt {
    color: var(--color-muted);
    text-transform: capitalize;
    white-space: nowrap;
  }

  .fact dd {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fact.lead dd {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
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
</style>

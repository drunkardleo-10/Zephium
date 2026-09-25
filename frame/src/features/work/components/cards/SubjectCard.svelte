<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import SubjectPicture from "./SubjectPicture.svelte";
  import HostGlyph from "./HostGlyph.svelte";
  import { displayHost } from "$shared/ui/data/Artifact";
  import { factBeside, SUBJECT_FACTS } from "../../lib/card-size";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const homepage = $derived(item.subject?.homepage ?? "");
  const host = $derived(displayHost(homepage));
  /** The lead fact (the price, when there is one) and the rest, each whole. */
  const facts = $derived((item.facts ?? []).slice(0, SUBJECT_FACTS));
</script>

{#snippet picture()}<div class="picture">
    <SubjectPicture picture={item.image} name={item.title} large />
  </div>{/snippet}
{#snippet mark()}<HostGlyph
    {host}
    url={homepage}
    icon={item.icon ?? null}
    size={20}
    initial={false}
  />{/snippet}

<!-- A picture when the run admitted one; otherwise the site's own mark beside the name. -->
<CardFrame
  id={item.id}
  title={item.title}
  {selected}
  unavailable={item.unavailable}
  hero={item.image ? picture : undefined}
  leading={item.image ? undefined : mark}
  dense
>
  {#if facts.length}
    <dl class="facts">
      {#each facts as fact, index (fact.label)}
        <div class="fact" class:lead={index === 0} class:stacked={!factBeside(fact)}>
          <dt>{fact.label}</dt>
          <dd>{fact.value}</dd>
        </div>
      {/each}
    </dl>
  {:else if item.detail}<p class="descriptor">{item.detail}</p>{/if}
  {#snippet footer()}<span class="host">{host || item.status}</span>{/snippet}
</CardFrame>

<style>
  /* The hero sits inside the card on its own inset corner. */
  .picture {
    box-sizing: border-box;
    block-size: 112px;
    padding: 4px 4px 0;
    background: var(--color-surface);
  }

  .picture > :global(*) {
    border-radius: var(--radius-inset);
    overflow: hidden;
  }

  .facts {
    display: grid;
    gap: 4px;
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

  .fact.stacked {
    flex-direction: column;
    align-items: stretch;
    gap: 0;
  }

  .fact dt {
    flex: none;
    color: var(--color-muted);
  }

  .fact.stacked dt {
    font-size: var(--text-caption);
    line-height: 13px;
  }

  .fact dd {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    min-inline-size: 0;
    margin: 0;
    overflow: hidden;
    overflow-wrap: anywhere;
    text-align: end;
  }

  .fact.stacked dd {
    text-align: start;
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

  .host {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>

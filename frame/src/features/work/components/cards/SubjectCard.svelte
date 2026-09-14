<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { displayHost } from "$shared/ui/data/Artifact";
  import { mediaUrl } from "$domain/resources";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const host = $derived(displayHost(item.subject?.homepage));
  const picture = $derived(item.image ? mediaUrl(item.image.profile, item.image.digest) : null);
  let failed = $state(false);
</script>

<CardFrame kind={item.kind} title={item.title} {selected} unavailable={item.unavailable} dense>
  {#snippet leading()}{#if picture && !failed}<img
        class="picture"
        src={picture}
        alt=""
        onerror={() => (failed = true)}
      />{:else}<span class="mark" aria-hidden="true">{item.title.slice(0, 1)}</span>{/if}{/snippet}
  {#if picture && !failed}<div class="hero"><img src={picture} alt={item.title} /></div>{/if}
  {#if item.detail}<p class="descriptor">{item.detail}</p>{/if}
  {#snippet footer()}<span>{host || item.status}</span>{/snippet}
</CardFrame>

<style>
  .picture {
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-sm);
    object-fit: cover;
  }

  .hero {
    margin-block-end: 6px;
    border-radius: var(--radius-sm);
    background: var(--color-fill);
    overflow: hidden;
  }

  .hero img {
    display: block;
    inline-size: 100%;
    max-block-size: 120px;
    object-fit: cover;
  }

  .mark {
    display: grid;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-sm);
    background: var(--color-accent-soft);
    color: var(--color-text);
    font-weight: 700;
    text-transform: uppercase;
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

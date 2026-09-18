<script lang="ts">
  import { untrack } from "svelte";
  import { mediaUrl } from "$domain/resources";
  import type { ComparePicture } from "../../lib/compare";
  let {
    picture,
    name,
    large = false,
  }: {
    /** The admitted picture, if the run got one; nothing is ever invented. */
    picture?: ComparePicture;
    name: string;
    large?: boolean;
  } = $props();
  const source = $derived(picture ? mediaUrl(picture.profile, picture.digest) : null);
  // A picture that will not load leaves the tile, never a broken glyph.
  let failed = $state<string | null>(null);
  $effect(() => {
    const current = source;
    untrack(() => {
      if (failed !== null && failed !== current) failed = null;
    });
  });
</script>

{#if source && failed !== source}
  <img src={source} alt="" decoding="async" draggable="false" onerror={() => (failed = source)} />
{:else}
  <span class="mark" class:large aria-hidden="true">{name.slice(0, 1)}</span>
{/if}

<style>
  img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
  }

  .mark {
    display: grid;
    place-items: center;
    inline-size: 100%;
    block-size: 100%;
    background: var(--color-fill);
    color: var(--color-faint);
    font-weight: 600;
    text-transform: uppercase;
  }

  .mark.large {
    font-size: var(--text-title);
  }
</style>

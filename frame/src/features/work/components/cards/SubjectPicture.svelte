<script lang="ts">
  import { getContext, onMount, untrack } from "svelte";
  import { mediaUrl } from "$domain/resources";
  import HostGlyph, { siteMark, siteOrigin } from "./HostGlyph.svelte";
  import { canvasProbe } from "../../lib/canvas-context";
  import type { ComparePicture } from "../../lib/compare";
  let {
    picture,
    name,
    large = false,
    homepage,
    markSize = 32,
  }: {
    /** The admitted picture, if the run got one; nothing is ever invented. */
    picture?: ComparePicture;
    name: string;
    large?: boolean;
    /** Where the subject lives: without a picture, its site's icon stands on the tile. */
    homepage?: string;
    /** The site icon's size on a tile without a picture. */
    markSize?: number;
  } = $props();
  const marked = $derived(homepage !== undefined && !!siteMark(homepage));
  // A site the cache does not hold is asked for once; its name stands meanwhile.
  const probe = getContext<((origin: string) => void) | undefined>(canvasProbe);
  onMount(() => {
    if (picture || homepage === undefined || marked || !probe) return;
    const origin = siteOrigin(homepage);
    if (origin) probe(origin);
  });
  const source = $derived(picture ? mediaUrl(picture.profile, picture.digest) : null);
  // A picture that will not load leaves a quiet plate: never a broken glyph, never an initial.
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
{:else if homepage !== undefined && marked}
  <span class="mark site" aria-hidden="true" title={name}
    ><HostGlyph url={homepage} size={markSize} initial={false} /></span
  >
{:else}
  <!-- No picture and no mark: the subject is its name, set, never a letter or a stock icon. -->
  <span class="mark named" class:large aria-hidden="true">{name}</span>
{/if}

<style>
  img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
  }

  .named {
    box-sizing: border-box;
    padding: 10px;
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    font-weight: 600;
    line-height: 1.3;
    text-align: center;
    text-wrap: balance;
    overflow-wrap: anywhere;
  }

  .named.large {
    font-size: var(--text-page-title);
  }

  .mark {
    display: grid;
    place-items: center;
    inline-size: 100%;
    block-size: 100%;
    background: var(--color-fill);
    color: var(--color-faint);
  }
</style>

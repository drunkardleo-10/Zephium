<script lang="ts">
  import { untrack } from "svelte";
  import { mediaUrl } from "$domain/resources";
  import Icon from "$shared/ui/Icon";
  import HostGlyph from "./HostGlyph.svelte";
  import { Image01Icon } from "../../lib/icons";
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
{:else if homepage !== undefined}
  <span class="mark site" aria-hidden="true" title={name}
    ><HostGlyph url={homepage} size={markSize} initial={false} /></span
  >
{:else}
  <span class="mark" aria-hidden="true" title={name}
    ><Icon icon={Image01Icon} size={large ? 20 : 14} /></span
  >
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
  }
</style>

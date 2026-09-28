<script module lang="ts">
  import { siteMark } from "../cards/HostGlyph.svelte";
  /** Whether a known mark stands for a host or an address right now. */
  export const hasMark = (address: string | undefined) => !!address && !!siteMark(address);
</script>

<script lang="ts">
  import { getContext, onMount } from "svelte";
  import FavIcon from "$shared/ui/FavIcon";
  import { siteOrigin } from "../cards/HostGlyph.svelte";
  import { canvasProbe } from "../../lib/canvas-context";
  /**
   * A site's or a product's own mark, or nothing at all: never a letter, never a
   * globe standing in for a logo the cache does not hold.
   */
  let { address, size = 16 }: { address: string; size?: number } = $props();
  const mark = $derived(siteMark(address));
  const probe = getContext<((origin: string) => void) | undefined>(canvasProbe);
  onMount(() => {
    if (mark || !probe) return;
    const origin = siteOrigin(address);
    if (origin) probe(origin);
  });
</script>

{#if mark}<span class="mark"><FavIcon image={mark.image} tone={mark.tone} {size} lit /></span>{/if}

<style>
  .mark {
    display: inline-flex;
    flex: none;
  }
</style>

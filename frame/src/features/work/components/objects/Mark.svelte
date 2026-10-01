<script module lang="ts">
  import { siteMark, siteOrigin } from "../cards/HostGlyph.svelte";
  import { registrableSite } from "../../lib/run/site";
  /** The site a subdomain belongs to (`docs.hetzner.com` → `hetzner.com`), or none. */
  function parentOf(address: string): string | null {
    const origin = siteOrigin(address);
    if (!origin) return null;
    const host = new URL(origin).hostname.replace(/^www\./iu, "");
    const site = registrableSite(host);
    return site && site !== host ? site : null;
  }
  /**
   * A site's mark: its own, else its maker's site's (a docs or app subdomain
   * often has no icon of its own).
   */
  function markOf(address: string) {
    const parent = parentOf(address);
    return siteMark(address) ?? (parent ? siteMark(parent) : null);
  }
  /** Whether a known mark stands for a host or an address right now. */
  export const hasMark = (address: string | undefined) => !!address && !!markOf(address);
</script>

<script lang="ts">
  import { getContext, onMount } from "svelte";
  import FavIcon from "$shared/ui/FavIcon";
  import { canvasProbe } from "../../lib/canvas-context";
  /**
   * A site's or a product's own mark, or nothing at all: never a letter, never a
   * globe standing in for a logo the cache does not hold.
   */
  let { address, size = 16 }: { address: string; size?: number } = $props();
  const mark = $derived(markOf(address));
  const probe = getContext<((origin: string) => void) | undefined>(canvasProbe);
  onMount(() => {
    if (mark || !probe) return;
    const origin = siteOrigin(address);
    if (origin) probe(origin);
    const parent = parentOf(address);
    if (parent) probe(`https://${parent}`);
  });
</script>

{#if mark}<span class="mark"><FavIcon image={mark.image} tone={mark.tone} {size} lit /></span>{/if}

<style>
  .mark {
    display: inline-flex;
    flex: none;
  }
</style>

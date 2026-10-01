<script module lang="ts">
  import { favicons } from "$domain/favicons";
  import type { SiteMark } from "$shared/ui/data/Artifact/site-marks";
  import { registrableSite } from "../../lib/run/site";
  /** The mark the favicon cache already holds for an address or a bare host; it asks native for nothing. */
  export function siteMark(address: string): SiteMark | null {
    const value = address.trim();
    if (!value) return null;
    let host = value;
    if (/^https?:\/\//iu.test(value)) {
      const exact = favicons.forPage(value);
      if (exact) return exact;
      try {
        host = new URL(value).host;
      } catch {
        return null;
      }
    }
    const bare = host.replace(/^www\./iu, "");
    const of = (name: string) =>
      favicons.forPage(`https://${name}`) ??
      favicons.forPage(`https://www.${name}`) ??
      favicons.forPage(`http://${name}`);
    // A subdomain with no icon of its own wears its site's: docs.hetzner.com is Hetzner's.
    const site = registrableSite(bare);
    return of(bare) ?? (site !== bare ? of(site) : null);
  }
  /** The HTTPS origin native may probe for an address or a bare host. */
  export function siteOrigin(address: string): string | null {
    const value = address.trim();
    if (!value) return null;
    try {
      const url = new URL(/^https?:\/\//iu.test(value) ? value : `https://${value}`);
      return url.protocol === "https:" && url.hostname.includes(".") ? url.origin : null;
    } catch {
      return null;
    }
  }
</script>

<script lang="ts">
  // The implementation file, not the Artifact module: a small lazy chunk that
  // draws one glyph must not pull the whole result renderer with it.
  import { getContext, onMount } from "svelte";
  import HostGlyph from "$shared/ui/data/Artifact/HostGlyph.svelte";
  import FavIcon from "$shared/ui/FavIcon";
  import type { IconRef } from "$shared/ipc/bindings";
  import { canvasProbe } from "../../lib/canvas-context";
  import { File01Icon } from "../../lib/icons";
  let {
    host = "",
    url = "",
    icon = null,
    file = false,
    size = 16,
    loading = false,
    initial: _initial = true,
  }: {
    host?: string;
    url?: string;
    /** A live tab's own reference, preferred over the cache lookup. */
    icon?: IconRef | null;
    file?: boolean;
    size?: number;
    /** The page is being read: the mark steps back under main's arc. */
    loading?: boolean;
    /** Kept for callers; without a mark nothing stands in, never a letter or a globe. */
    initial?: boolean;
  } = $props();
  const held = $derived(icon ? favicons.image(icon) : null);
  const mark = $derived<SiteMark | null>(
    file ? null : held ? { image: held, tone: favicons.tone(icon) } : siteMark(url || host),
  );
  // A site the cache does not hold is asked for once per canvas; the glyph stands meanwhile.
  const probe = getContext<((origin: string) => void) | undefined>(canvasProbe);
  onMount(() => {
    if (file || mark || !probe) return;
    const origin = siteOrigin(url || host);
    if (origin) probe(origin);
  });
</script>

{#if mark || loading}<FavIcon
    image={mark?.image ?? null}
    tone={mark?.tone ?? "mid"}
    {size}
    {loading}
    fallback={file ? File01Icon : undefined}
  />{:else if file}<HostGlyph {host} {file} {size} />{/if}

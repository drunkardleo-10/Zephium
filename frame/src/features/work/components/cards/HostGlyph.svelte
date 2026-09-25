<script module lang="ts">
  import { favicons } from "$domain/favicons";
  import type { SiteMark } from "$shared/ui/data/Artifact/site-marks";
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
    return (
      favicons.forPage(`https://${bare}`) ??
      favicons.forPage(`https://www.${bare}`) ??
      favicons.forPage(`http://${bare}`)
    );
  }
</script>

<script lang="ts">
  // The implementation file, not the Artifact module: a small lazy chunk that
  // draws one glyph must not pull the whole result renderer with it.
  import HostGlyph from "$shared/ui/data/Artifact/HostGlyph.svelte";
  import FavIcon from "$shared/ui/FavIcon";
  import type { IconRef } from "$shared/ipc/bindings";
  import { File01Icon, GlobalIcon } from "../../lib/icons";
  let {
    host = "",
    url = "",
    icon = null,
    file = false,
    size = 16,
    loading = false,
    initial = true,
  }: {
    host?: string;
    url?: string;
    /** A live tab's own reference, preferred over the cache lookup. */
    icon?: IconRef | null;
    file?: boolean;
    size?: number;
    /** The page is being read: the mark steps back under main's arc. */
    loading?: boolean;
    /** Without a mark, a letter stands in; a subject or a page takes a neutral globe instead. */
    initial?: boolean;
  } = $props();
  const held = $derived(icon ? favicons.image(icon) : null);
  const mark = $derived<SiteMark | null>(
    file ? null : held ? { image: held, tone: favicons.tone(icon) } : siteMark(url || host),
  );
</script>

{#if mark || loading || !initial}<FavIcon
    image={mark?.image ?? null}
    tone={mark?.tone ?? "mid"}
    {size}
    {loading}
    fallback={file ? File01Icon : GlobalIcon}
  />{:else}<HostGlyph {host} {file} {size} />{/if}

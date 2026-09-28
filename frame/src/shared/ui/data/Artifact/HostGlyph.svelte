<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import FavIcon from "$shared/ui/FavIcon";
  import File01Icon from "@hugeicons/core-free-icons/File01Icon";
  import { siteMarks } from "./site-marks";
  /** A site's real mark when its owner holds one, a file's glyph for a file, else nothing. */
  let {
    host = "",
    url = "",
    file = false,
    size = 16,
  }: { host?: string; url?: string; file?: boolean; size?: number } = $props();
  const resolve = siteMarks();
  const mark = $derived(!file && resolve ? resolve(url || host) : null);
</script>

{#if mark}<FavIcon image={mark.image} tone={mark.tone} {size} />{:else if file}<span
    class="glyph"
    style:--glyph={`${size}px`}
    aria-hidden="true"><Icon icon={File01Icon} size={Math.round(size * 0.62)} /></span
  >{/if}

<style>
  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: var(--glyph);
    block-size: var(--glyph);
    border-radius: calc(var(--glyph) * 0.3);
    background: var(--color-fill);
    color: var(--color-faint);
  }
</style>

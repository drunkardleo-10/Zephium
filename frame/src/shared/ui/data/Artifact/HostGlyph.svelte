<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import FavIcon from "$shared/ui/FavIcon";
  import File01Icon from "@hugeicons/core-free-icons/File01Icon";
  import { siteMarks } from "./site-marks";
  /** A site's real mark when its owner holds one; otherwise a neutral stand-in. */
  let {
    host = "",
    url = "",
    file = false,
    size = 16,
  }: { host?: string; url?: string; file?: boolean; size?: number } = $props();
  const resolve = siteMarks();
  const mark = $derived(!file && resolve ? resolve(url || host) : null);
  const letter = $derived(
    host
      .replace(/^www\./u, "")
      .slice(0, 1)
      .toLocaleUpperCase(),
  );
</script>

{#if mark}<FavIcon image={mark.image} tone={mark.tone} {size} />{:else}<span
    class="glyph"
    class:file
    style:--glyph={`${size}px`}
    aria-hidden="true"
  >
    {#if file}<Icon icon={File01Icon} size={Math.round(size * 0.62)} />{:else}{letter}{/if}
  </span>{/if}

<style>
  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: var(--glyph);
    block-size: var(--glyph);
    border-radius: 50%;
    background: var(--color-fill);
    color: var(--color-faint);
    font-size: max(9px, calc(var(--glyph) * 0.56));
    font-weight: 600;
    line-height: 1;
  }

  .glyph.file {
    border-radius: calc(var(--glyph) * 0.3);
  }
</style>

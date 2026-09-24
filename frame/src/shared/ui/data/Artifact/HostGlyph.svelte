<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import File01Icon from "@hugeicons/core-free-icons/File01Icon";
  /** A neutral stand-in for a site's mark; a real favicon fills this slot later. */
  let {
    host = "",
    file = false,
    size = 16,
  }: { host?: string; file?: boolean; size?: number } = $props();
  const letter = $derived(
    host
      .replace(/^www\./u, "")
      .slice(0, 1)
      .toLocaleUpperCase(),
  );
</script>

<span class="glyph" class:file style:--glyph={`${size}px`} aria-hidden="true">
  {#if file}<Icon icon={File01Icon} size={Math.round(size * 0.62)} />{:else}{letter}{/if}
</span>

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

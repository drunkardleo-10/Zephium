<script lang="ts">
  /** A neutral stand-in for a site's mark; a real favicon fills this slot later. */
  let { host = "", size = 16 }: { host?: string; size?: number } = $props();
  const letter = $derived(
    host
      .replace(/^www\./u, "")
      .slice(0, 1)
      .toLocaleUpperCase(),
  );
</script>

<span class="glyph" style:--glyph={`${size}px`} aria-hidden="true">{letter}</span>

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
    font-size: calc(var(--glyph) * 0.56);
    font-weight: 600;
    line-height: 1;
  }
</style>

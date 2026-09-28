<script lang="ts">
  /** A filled star, the one glyph a rating and a top pick share. */
  let { size = 12 }: { size?: number } = $props();
</script>

<svg viewBox="0 0 12 12" width={size} height={size} aria-hidden="true"
  ><path d="M6 .9 7.5 4.1l3.5.4-2.6 2.4.7 3.5L6 8.6 2.9 10.4l.7-3.5L1 4.5l3.5-.4z" /></svg
>

<style>
  svg {
    flex: none;
    fill: currentcolor;
  }
</style>

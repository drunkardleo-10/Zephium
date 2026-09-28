<script lang="ts">
  /** The play mark on a poster: a lit disc with its triangle. */
  let { size = 44 }: { size?: number } = $props();
</script>

<svg viewBox="0 0 44 44" width={size} height={size} aria-hidden="true"
  ><circle cx="22" cy="22" r="22" class="disc" /><path
    d="M18 14.5v15a1 1 0 0 0 1.5.86l12.4-7.5a1 1 0 0 0 0-1.72l-12.4-7.5A1 1 0 0 0 18 14.5z"
    class="play"
  /></svg
>

<style>
  svg {
    flex: none;
    border-radius: var(--radius-capsule);
    box-shadow: var(--shadow-float);
  }

  .disc {
    fill: var(--color-lit);
  }

  .play {
    fill: var(--color-on-lit);
  }
</style>

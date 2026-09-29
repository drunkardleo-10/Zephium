<script lang="ts">
  /**
   * The play mark on a poster: a lit disc with its triangle, or, on a YouTube
   * video, YouTube's own rounded mark, so the poster says where it plays.
   */
  let { size = 44, youtube = false }: { size?: number; youtube?: boolean } = $props();
</script>

{#if youtube}<svg
    class="youtube"
    viewBox="0 0 68 48"
    width={size}
    height={(size * 48) / 68}
    aria-hidden="true"
    ><path
      class="plate"
      d="M66.5 7.5A8.5 8.5 0 0 0 60.5 1.5C55.2 0 34 0 34 0S12.8 0 7.5 1.5A8.5 8.5 0 0 0 1.5 7.5C0 12.8 0 24 0 24s0 11.2 1.5 16.5a8.5 8.5 0 0 0 6 6C12.8 48 34 48 34 48s21.2 0 26.5-1.5a8.5 8.5 0 0 0 6-6C68 35.2 68 24 68 24s0-11.2-1.5-16.5z"
    /><path class="play" d="M27 34.3 44.8 24 27 13.7z" /></svg
  >{:else}<svg class="disc" viewBox="0 0 44 44" width={size} height={size} aria-hidden="true"
    ><circle cx="22" cy="22" r="22" class="plate" /><path
      d="M18 14.5v15a1 1 0 0 0 1.5.86l12.4-7.5a1 1 0 0 0 0-1.72l-12.4-7.5A1 1 0 0 0 18 14.5z"
      class="play"
    /></svg
  >{/if}

<style>
  svg {
    flex: none;
  }

  .disc {
    border-radius: var(--radius-capsule);
    box-shadow: var(--shadow-float);
  }

  .disc .plate {
    fill: var(--color-lit);
  }

  .disc .play {
    fill: var(--color-on-lit);
  }

  .youtube .plate {
    fill: var(--color-youtube);
  }

  .youtube .play {
    fill: var(--color-on-youtube);
  }
</style>

<script lang="ts">
  /** A score as filled dots on its scale: ten-point scales read in fives. */
  let { value, max, label }: { value: number; max: number; label: string } = $props();
  const scale = $derived(max > 5 ? 5 : max);
  const filled = $derived((value / max) * scale);
</script>

<span class="dots" role="img" aria-label={label} title={label}>
  {#each Array.from({ length: scale }, (_, index) => index) as index (index)}
    <span
      class="dot"
      class:on={filled >= index + 0.75}
      class:half={filled >= index + 0.25 && filled < index + 0.75}
    ></span>
  {/each}
</span>

<style>
  .dots {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    vertical-align: middle;
  }

  .dot {
    inline-size: 7px;
    block-size: 7px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-strong);
  }

  .dot.on {
    background: var(--color-text);
  }

  .dot.half {
    background: linear-gradient(90deg, var(--color-text) 50%, var(--color-fill-strong) 50%);
  }
</style>

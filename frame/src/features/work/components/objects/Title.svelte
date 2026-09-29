<script lang="ts">
  /** An object's name: set large enough to read when the canvas is surveyed; never cut mid-word. */
  let { text, level = 3 }: { text: string; level?: 2 | 3 } = $props();
</script>

<svelte:element this={`h${level}`} class="title">{text}</svelte:element>

<style>
  .title {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-object-title);
    font-weight: 650;
    line-height: 1.25;
    letter-spacing: -0.012em;
    text-wrap: balance;
  }
</style>

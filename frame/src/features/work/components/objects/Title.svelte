<script lang="ts">
  import type { Detail } from "../../lib/board/types";
  /** An object's name, set to be read at its level of detail; never cut mid-word. */
  let { text, detail, level = 3 }: { text: string; detail: Detail; level?: 2 | 3 } = $props();
</script>

<svelte:element this={`h${level}`} class="title {detail}">{text}</svelte:element>

<style>
  .title {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-page-title);
    font-weight: 600;
    line-height: 20px;
    letter-spacing: -0.005em;
    text-wrap: balance;
  }

  .overview {
    font-size: var(--text-overview-title);
    line-height: 1.2;
    letter-spacing: -0.015em;
  }

  .tile {
    font-size: var(--text-tile-title);
    line-height: 1.12;
    letter-spacing: -0.02em;
  }
</style>

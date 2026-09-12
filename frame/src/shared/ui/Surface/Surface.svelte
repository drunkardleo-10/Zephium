<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    children,
    elevation = "surface",
    class: className = "",
  }: {
    children: Snippet;
    elevation?: "canvas" | "surface" | "raised" | "overlay";
    class?: string;
  } = $props();
</script>

<div class={["ui-surface", className]} data-elevation={elevation}>{@render children()}</div>

<style>
  /* Surfaces step by tone. Only overlay leaves the page. */
  .ui-surface {
    box-sizing: border-box;
    min-width: 0;
    padding: 18px;
    border-radius: var(--radius-card);
    background: var(--color-card);
    box-shadow: 0 0 0 1px var(--color-border);
  }

  .ui-surface[data-elevation="canvas"] {
    background: transparent;
    box-shadow: none;
  }

  .ui-surface[data-elevation="raised"] {
    background: var(--color-raised);
    box-shadow:
      0 0 0 1px var(--color-border),
      var(--shadow-raised);
  }

  .ui-surface[data-elevation="overlay"] {
    background: var(--color-menu);
    box-shadow: var(--shadow-popover);
  }

  @media (forced-colors: active) {
    .ui-surface {
      border: 1px solid ButtonText;
      box-shadow: none;
    }
  }
</style>

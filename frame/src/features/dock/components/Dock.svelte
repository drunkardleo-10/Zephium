<script lang="ts">
  import type { Snippet } from "svelte";
  import ToolShelf from "./ToolShelf.svelte";

  let { sites, compact = false }: { sites?: Snippet; compact?: boolean } = $props();
</script>

<!--
  The base of the column is one row: the mini-apps lead it and the kept sites
  fill the rest of it. They are the same material at the same height, so what
  sits down here reads as a single strip of things you can launch rather than
  as a control bar with a list bolted underneath.
-->
<footer class="dock" data-compact={compact}>
  <ToolShelf {compact} />
  <span class="dock-rule" aria-hidden="true"></span>
  {#if sites}<div class="dock-sites-slot">{@render sites()}</div>{/if}
</footer>

<style>
  .dock {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--dock-shelf-gap);
    margin-block-start: 8px;
    padding: 0 var(--sidebar-inset) var(--sidebar-inset);
  }

  /* Ours on one side of the rule, the web's on the other. */
  .dock-rule {
    flex: none;
    align-self: center;
    width: 1px;
    height: calc(var(--dock-tile) - 14px);
    border-radius: var(--radius-capsule);
    background: var(--color-border);
  }

  .dock-sites-slot {
    flex: 1;
    min-width: 0;
  }

  /* The rail stacks the same three parts it does at full width. */
  .dock[data-compact="true"] {
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding-block-end: 8px;
  }

  .dock[data-compact="true"] .dock-rule {
    width: 22px;
    height: 1px;
  }

  .dock[data-compact="true"] .dock-sites-slot {
    flex: none;
  }
</style>

<script lang="ts">
  import type { Snippet } from "svelte";
  import ToolShelf from "./ToolShelf.svelte";

  let {
    sites,
    above,
    compact = false,
    tools = true,
  }: {
    sites?: Snippet;
    /** Kept sites that did not fit beside the shelf, in rows over it. */
    above?: Snippet;
    compact?: boolean;
    /** The browser's tool case; Work keeps its own tools on the canvas. */
    tools?: boolean;
  } = $props();
</script>

<!--
  The base of the column. Its bottom row is the mini-apps and as many kept
  sites as fit beside them; the rest of the sites stack in full-width rows
  above, growing upwards, so the shelf never moves and the column above only
  gives up a row when a whole row has been filled.
-->
<!-- The base of the column settles last, once the list above it has. -->
<footer class="dock" data-compact={compact} data-cascade style:--cascade={8}>
  {#if above}{@render above()}{/if}
  <div class="dock-base">
    {#if tools}<ToolShelf {compact} />
      <span class="dock-rule" aria-hidden="true"></span>{/if}
    {#if sites}<div class="dock-sites-slot">{@render sites()}</div>{/if}
  </div>
</footer>

<style>
  .dock {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: var(--dock-gap);
    margin-block-start: 8px;
    padding: 0 var(--sidebar-inset) var(--sidebar-inset);
  }

  .dock-base {
    display: flex;
    align-items: center;
    gap: var(--dock-shelf-gap);
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
    align-items: center;
    gap: 8px;
    padding-block-end: 8px;
  }

  .dock[data-compact="true"] .dock-base {
    flex-direction: column;
    gap: 8px;
  }

  .dock[data-compact="true"] .dock-rule {
    width: 22px;
    height: 1px;
  }

  .dock[data-compact="true"] .dock-sites-slot {
    flex: none;
  }
</style>

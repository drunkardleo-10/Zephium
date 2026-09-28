<script lang="ts">
  import type { Snippet } from "svelte";
  import { onDestroy } from "svelte";
  import {
    COMPACT_WIDTH,
    effectiveWidth,
    expandBriefly,
    hasPanel,
  } from "$session/sidebar-mode.svelte";

  let {
    labelledby,
    describedby,
    children,
  }: { labelledby: string; describedby: string; children: Snippet } = $props();

  const INSET = 8;

  // The page is a native view drawn over the window, so a prompt in the
  // middle of it would sit underneath. It belongs to the sidebar instead: at
  // rail width the full column opens for it and folds away after, and beside
  // an open tool it covers the tool's panel.
  const beside = hasPanel();
  const restore = beside ? () => {} : expandBriefly();
  onDestroy(restore);

  let left = $derived(beside ? COMPACT_WIDTH + INSET : INSET);
</script>

<div class="scrim" style:width={`${effectiveWidth()}px`} aria-hidden="true"></div>
<div
  role="alertdialog"
  aria-modal="true"
  aria-labelledby={labelledby}
  aria-describedby={describedby}
  class="sheet"
  style:left={`${left}px`}
  style:width={`${Math.max(effectiveWidth() - left - INSET, 220)}px`}
>
  {@render children()}
</div>

<style>
  .scrim {
    position: fixed;
    inset-block: 0;
    inset-inline-start: 0;
    z-index: 50;
    background: color-mix(in srgb, var(--color-canvas) 72%, transparent);
    animation: interface-fade var(--motion-fast) var(--ease-out) both;
  }

  .sheet {
    position: fixed;
    top: 50%;
    translate: 0 -50%;
    z-index: 51;
    box-sizing: border-box;
    max-height: calc(100vh - 56px);
    overflow-y: auto;
    padding: 16px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-panel);
    background: var(--color-raised);
    box-shadow: var(--shadow-overlay);
    text-align: start;
    animation: sheet-in var(--motion-slow) var(--ease-out) both;
  }

  @keyframes sheet-in {
    from {
      opacity: 0;
      scale: 0.97;
    }
  }
</style>

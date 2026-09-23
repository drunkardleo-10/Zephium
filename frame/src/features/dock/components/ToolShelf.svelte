<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { ToolCaseIcon } from "@hugeicons/core-free-icons";
  import { commands } from "$shared/ipc/bindings";
  import * as tools from "$session/tools.svelte";
  import Icon from "$shared/ui/Icon";
  import { SHELF_TOOLS, toolPresentation } from "../lib/dock-tools";

  let { compact = false }: { compact?: boolean } = $props();

  let open = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  // Leaving is deferred so a pointer crossing a corner of the shelf does not
  // dismiss what it was travelling towards; entering cancels the pending one.
  // The tile listens as well as the group: picking a tool hides the stack
  // under the pointer, and the group never sees it leave.
  function reveal() {
    clearTimeout(timer);
    open = true;
  }
  function conceal() {
    clearTimeout(timer);
    timer = setTimeout(() => (open = false), 90);
  }
  $effect(() => () => clearTimeout(timer));

  function openMenu(event: MouseEvent) {
    clearTimeout(timer);
    open = false;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    void commands.toolsMenuPopup(rect.left, rect.top);
  }

  function pick(kind: (typeof SHELF_TOOLS)[number]) {
    clearTimeout(timer);
    open = false;
    if (tools.activeTool() === kind) tools.close();
    else tools.open(kind);
  }
</script>

<!--
  A tile at the head of the site row, built from the same material as the
  sites beside it: the mini-apps are things you launch, so they keep the
  company of the other things you launch rather than sitting in a control
  bar above them.

  Hovering it grows them out of the tile, named and vertical, near-to-far so
  the stack reads as coming from the tile rather than appearing beside it;
  clicking hands the whole list to the native menu. Focus opens the same
  stack, so the keyboard reaches them by tabbing forward out of the tile.

  There is no card behind them. A panel would claim a region of the column
  the tools do not need; each one carries its own ground instead, so what
  arrives is the tools themselves and nothing else.
-->
<div
  class="shelf"
  data-compact={compact}
  role="group"
  aria-label={m.dock_tools()}
  onpointerenter={reveal}
  onpointerleave={conceal}
  onfocusin={reveal}
  onfocusout={conceal}
>
  <button
    type="button"
    class="case"
    class:case-lit={tools.activeTool() !== null}
    aria-label={m.dock_tools()}
    title={m.dock_tools()}
    aria-haspopup="menu"
    onpointerenter={reveal}
    onclick={openMenu}
  >
    <Icon icon={ToolCaseIcon} size={18} />
  </button>
  <div class="flyout" data-open={open} inert={!open}>
    {#each SHELF_TOOLS as kind, index (kind)}
      {@const view = toolPresentation(kind)}
      <button
        type="button"
        class="tool"
        style:--step={SHELF_TOOLS.length - 1 - index}
        aria-pressed={tools.activeTool() === kind}
        onclick={() => pick(kind)}
      >
        <Icon icon={view.icon} size={14} />
        {#if !compact}<span>{view.label()}</span>{/if}
      </button>
    {/each}
  </div>
</div>

<style>
  .shelf {
    position: relative;
    z-index: 20;
    flex: none;
  }

  /* The one square in a row of wide tiles: same height, same radius, so it
     keeps the row's rhythm — but sunk into the column instead of sitting on
     it. The sites are plates carrying someone else's mark; this is a well cut
     into our own chrome, which is the difference you should be able to see
     without reading either of them. */
  .case {
    display: grid;
    place-items: center;
    width: var(--dock-tile);
    height: var(--dock-tile);
    border: 0;
    border-radius: var(--radius-card);
    background: var(--color-fill);
    box-shadow: var(--shadow-track);
    color: var(--color-muted);
    cursor: default;
    outline: none;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      scale var(--motion-slow) var(--ease-spring);
  }

  .case:hover {
    color: var(--color-text);
  }

  .case:active {
    scale: 0.94;
    transition-duration: var(--motion-instant);
  }

  /* With a tool open the well fills and rises: the thing you opened is now
     standing proud of the column it came out of. */
  .case-lit {
    background: var(--row-active);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
  }

  /*
    The stack floats clear of the column rather than pushing it, and carries
    a bridge over the gap back to the case so the pointer can travel between
    them without crossing dead ground. It is a layout box only: every pixel
    that gets painted belongs to one of the tools.
  */
  .flyout {
    position: absolute;
    bottom: calc(100% + 8px);
    inset-inline-start: 0;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 4px;
    width: max-content;
    visibility: hidden;
    pointer-events: none;
    transition: visibility 0s linear var(--motion-fast);
  }

  .flyout::after {
    content: "";
    position: absolute;
    inset-inline: 0;
    top: 100%;
    height: 10px;
  }

  .flyout[data-open="true"] {
    visibility: visible;
    pointer-events: auto;
    transition: visibility 0s;
  }

  /* Each one is the same plate as the tiles below — the essentials' own
     ground and ring — so the stack reads as the shelf opening rather than as
     a menu arriving over it. That ground is translucent, so it is laid over
     the chrome's own colour to stay opaque above the tab list, and it takes
     the tiles' raised shadow because here it really is above something. */
  .tool {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding-inline: 9px 12px;
    border: 0;
    border-radius: var(--radius-row);
    background: linear-gradient(var(--color-card), var(--color-card)), var(--color-chrome);
    box-shadow:
      inset 0 0 0 1px var(--color-border),
      var(--shadow-raised);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    letter-spacing: -0.004em;
    white-space: nowrap;
    text-align: start;
    cursor: default;
    outline: none;
    transform-origin: bottom left;
    opacity: 0;
    scale: 0.9;
    translate: 0 8px;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      opacity var(--motion-fast) var(--ease-exit),
      scale var(--motion-fast) var(--ease-exit),
      translate var(--motion-fast) var(--ease-exit);
  }

  .tool:hover {
    background: linear-gradient(var(--row-hover), var(--row-hover)), var(--color-chrome);
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      var(--shadow-raised);
    color: var(--color-text);
  }

  .tool:active {
    scale: 0.96;
    transition-duration: var(--motion-instant);
  }

  /* The open tool is named by its glyph taking the accent, which is quieter
     than a second lit plate inside a stack that is already floating. */
  .tool[aria-pressed="true"] {
    color: var(--color-text);
  }

  .tool[aria-pressed="true"] :global(svg) {
    color: var(--color-accent);
  }

  .flyout[data-open="true"] .tool {
    opacity: 1;
    scale: 1;
    translate: 0 0;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      opacity var(--motion-base) var(--ease-smooth) calc(var(--step) * 30ms),
      scale var(--motion-slow) var(--ease-spring) calc(var(--step) * 30ms),
      translate var(--motion-slow) var(--ease-spring) calc(var(--step) * 30ms);
  }

  /* At rail width a name has nowhere to go, so the stack is glyphs only and
     each pill becomes the square its tile already is. */

  /* The rail's rhythm is 40px, not the dock tile's 44, and everything in it
     is a disc: at rail width a column of circles is what reads as a rail
     rather than as a squeezed list. */
  .shelf[data-compact="true"] .case {
    width: 40px;
    height: 40px;
    border-radius: var(--radius-capsule);
  }

  .shelf[data-compact="true"] .flyout {
    align-items: center;
    inset-inline-start: 50%;
    gap: 4px;
    translate: -50% 0;
  }

  .shelf[data-compact="true"] .tool {
    justify-content: center;
    width: 32px;
    height: 32px;
    padding-inline: 0;
    border-radius: var(--radius-capsule);
  }

  @media (prefers-reduced-motion: reduce) {
    .tool {
      transition-duration: 1ms;
      transition-delay: 0s;
      scale: 1;
      translate: none;
    }
  }

  @media (forced-colors: active) {
    .case-lit,
    .tool[aria-pressed="true"] {
      border: 1px solid Highlight;
    }
  }
</style>

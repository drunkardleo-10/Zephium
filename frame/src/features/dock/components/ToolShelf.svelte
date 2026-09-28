<script lang="ts">
  import type { Snippet } from "svelte";
  import * as m from "$shared/i18n/messages";
  import { ToolCaseIcon } from "@hugeicons/core-free-icons";
  import { commands } from "$shared/ipc/bindings";
  import * as tools from "$session/tools.svelte";
  import Disclosure from "$shared/ui/Disclosure";
  import Icon from "$shared/ui/Icon";
  import { RECORD_TOOLS, SHELF_TOOLS, toolPresentation } from "../lib/dock-tools";

  let { compact = false, extensions }: { compact?: boolean; extensions?: Snippet } = $props();

  const groups = [SHELF_TOOLS, RECORD_TOOLS] as const;
  const count = SHELF_TOOLS.length + RECORD_TOOLS.length;

  function pick(kind: (typeof SHELF_TOOLS)[number] | (typeof RECORD_TOOLS)[number]) {
    if (tools.activeTool() === kind) tools.close();
    else tools.open(kind);
  }

  // The full native menu stays one gesture away, where a Mac user reaches for
  // everything else a control can do.
  function nativeMenu(event: MouseEvent) {
    event.preventDefault();
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    void commands.toolsMenuPopup(rect.left, rect.top);
  }
</script>

<!--
  The head of the dock: the tools, stacked on demand. A click opens them as a
  menu rising out of the case, nearest first, the way a stack opens from the
  Dock; nothing opens on hover, because nothing on this platform does. At
  rail width a name has nowhere to go, so the stack is glyphs, each named by
  its tooltip.
-->
<div
  class="shelf"
  data-compact={compact}
  data-morph="tools"
  role="presentation"
  oncontextmenu={nativeMenu}
>
  <Disclosure
    label={m.dock_tools()}
    menu
    triggerClass={["case", tools.activeTool() !== null && "case-lit"].filter(Boolean).join(" ")}
    panelClass="shelf-stack"
  >
    {#snippet trigger()}<Icon icon={ToolCaseIcon} size={18} />{/snippet}
    {#if extensions}
      <!-- The web's buttons sit farthest from the case, apart from ours. -->
      <div class="shelf-extensions" style:--step={count}>{@render extensions()}</div>
      <div class="shelf-rule shelf-extensions-rule" role="separator"></div>
    {/if}
    {#each groups as group, at (at)}
      {#if at > 0}<div class="shelf-rule" role="separator"></div>{/if}
      {#each group as kind, index (kind)}
        {@const view = toolPresentation(kind)}
        {@const step = count - 1 - (at === 0 ? index : SHELF_TOOLS.length + index)}
        <button
          type="button"
          role="menuitem"
          class="ui-menu-item shelf-item"
          style:--step={step}
          title={compact ? view.label() : undefined}
          aria-label={compact ? view.label() : undefined}
          aria-current={tools.activeTool() === kind || undefined}
          onclick={() => pick(kind)}
        >
          <span class="ui-menu-icon"><Icon icon={view.icon} size={16} /></span>
          {#if !compact}<span>{view.label()}</span>{/if}
        </button>
      {/each}
    {/each}
  </Disclosure>
</div>

<style>
  .shelf {
    position: relative;
    z-index: 20;
    flex: none;
  }

  /* The one square at the head of the row, on the same ground as the kept
     sites beside it: ours on one side of the rule, the web's on the other. */
  .shelf :global(.case) {
    display: grid;
    place-items: center;
    width: var(--dock-tile);
    height: var(--dock-tile);
    border: 0;
    border-radius: var(--radius-card);
    background: var(--color-card);
    color: var(--color-muted);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      scale var(--motion-slow) var(--ease-spring);
  }

  .shelf :global(.case:hover),
  .shelf :global(.case[data-state="open"]) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .shelf :global(.case:active) {
    scale: 0.95;
    transition-duration: var(--motion-instant);
  }

  /* With a tool open the case is lit like any current thing in the column. */
  .shelf :global(.case.case-lit) {
    background: var(--color-fill-active);
    box-shadow: var(--row-rim);
    color: var(--color-text);
  }

  /* The stack rises out of the case rather than dropping from it. */
  .shelf :global(.shelf-stack) {
    top: auto;
    bottom: calc(100% + 8px);
    min-width: 176px;
    transform-origin: bottom left;
  }

  .shelf :global(.shelf-stack:not([data-open="true"])) {
    translate: 0 6px;
  }

  .shelf :global(.shelf-item) {
    width: 100%;
    border: 0;
    background: transparent;
    font: inherit;
    font-size: var(--text-body);
    text-align: start;
  }

  .shelf :global(.shelf-item:hover),
  .shelf :global(.shelf-item:focus-visible) {
    outline: none;
    background: var(--row-active);
  }

  .shelf :global(.shelf-item:active) {
    background: var(--row-pressed);
  }

  .shelf :global(.shelf-item[aria-current="true"] .ui-menu-icon) {
    color: var(--color-accent);
  }

  /* Each row settles in behind the one below it, so the stack reads as
     unfolding from the case instead of arriving as a block. */
  .shelf :global(.shelf-stack[data-open="true"] .shelf-item) {
    animation: shelf-item-in var(--motion-slow) var(--ease-out) both;
    animation-delay: calc(var(--step) * 16ms);
  }

  /* stylelint-disable-next-line keyframes-name-pattern -- Svelte's global prefix. */
  @keyframes -global-shelf-item-in {
    from {
      opacity: 0;
      translate: 0 6px;
    }
  }

  .shelf-extensions {
    display: flex;
    flex-direction: column;
  }

  /* Nothing to set apart when no extension has a button. */
  .shelf-extensions:not(:has(:global(button))),
  .shelf-extensions:not(:has(:global(button))) + .shelf-extensions-rule {
    display: none;
  }

  .shelf-rule {
    height: 1px;
    margin: 5px var(--menu-item-inset);
    background: var(--color-menu-separator);
  }

  /* At rail width: a square plate like every other rail item, and a stack
     of glyphs just wide enough for them. */
  .shelf[data-compact="true"] :global(.case) {
    width: 40px;
    height: var(--row-sidebar);
    border-radius: var(--radius-row);
  }

  /* Centred on the 40px case: four points either side of it. */
  .shelf[data-compact="true"] :global(.shelf-stack) {
    inset-inline-start: -4px;
    min-width: 0;
    width: 48px;
  }

  .shelf[data-compact="true"] :global(.shelf-item) {
    justify-content: center;
    padding-inline: 0;
  }

  .shelf[data-compact="true"] .shelf-rule {
    margin-inline: 8px;
  }

  @media (forced-colors: active) {
    .shelf :global(.case.case-lit) {
      border: 1px solid Highlight;
    }
  }
</style>

<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import {
    AirplaneTakeOff01Icon,
    Calendar03Icon,
    CheckmarkCircle02Icon,
    Doc01Icon,
    Home01Icon,
    Money03Icon,
    PassportIcon,
  } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const GLYPHS = {
    dates: Calendar03Icon,
    flight: AirplaneTakeOff01Icon,
    stay: Home01Icon,
    entry: PassportIcon,
    money: Money03Icon,
    document: Doc01Icon,
    check: CheckmarkCircle02Icon,
  } as const;
  const glyph = $derived(GLYPHS[item.step?.icon ?? "check"]);
</script>

<!-- One thing to do, whole: what it is at a glance, then its words. -->
<article class="step" class:selected data-icon={item.step?.icon ?? "check"}>
  <div class="work-drag-handle grip"></div>
  <header>
    <span class="tile" aria-hidden="true"><Icon icon={glyph} size={15} /></span>
    {#if item.step}<span class="index">{item.step.index}</span>{/if}
  </header>
  <p class="text">{item.step?.text ?? item.title}</p>
  {#if item.detail}<p class="detail">{item.detail}</p>{/if}
</article>

<style>
  .step {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 8px;
    box-sizing: border-box;
    block-size: 100%;
    padding: 14px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-text);
    overflow: hidden;
    transition: box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .step:hover {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .step.selected {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      0 0 0 2px var(--color-accent-soft),
      0 0 0 3px var(--color-accent);
  }

  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__node.dragging) .step {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      var(--shadow-popover);
  }

  .grip {
    position: absolute;
    inset: 0;
    cursor: grab;
  }

  .grip:active {
    cursor: grabbing;
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    flex: none;
    pointer-events: none;
  }

  .tile {
    display: grid;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-row);
    background: var(--color-accent-soft);
    color: var(--color-accent);
  }

  .index {
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    line-height: 13px;
  }

  .text,
  .detail {
    position: relative;
    margin: 0;
    overflow-wrap: anywhere;
    text-wrap: pretty;
    pointer-events: none;
  }

  .text {
    font-size: var(--text-label);
    font-weight: 500;
    line-height: 16px;
  }

  .detail {
    margin-block-start: -4px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 15px;
  }
</style>

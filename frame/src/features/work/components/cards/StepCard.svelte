<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import CardFrame from "./CardFrame.svelte";
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
<CardFrame id={item.id} {selected} plain>
  <div class="step" data-icon={item.step?.icon ?? "check"}>
    <header>
      <span class="tile" aria-hidden="true"><Icon icon={glyph} size={15} /></span>
      {#if item.step}<span class="index">{item.step.index}</span>{/if}
    </header>
    <p class="text">{item.step?.text ?? item.title}</p>
    {#if item.detail}<p class="detail">{item.detail}</p>{/if}
  </div>
</CardFrame>

<style>
  .step {
    display: flex;
    flex-direction: column;
    gap: 8px;
    box-sizing: border-box;
    block-size: 100%;
    min-block-size: 0;
    padding: 12px 12px 16px;
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    flex: none;
  }

  .tile {
    display: grid;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-label-secondary);
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
    margin: 0;
    overflow-wrap: anywhere;
    text-wrap: pretty;
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

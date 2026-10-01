<script lang="ts">
  import type { TimelineBlock } from "../../lib/board/types";
  let { block, width }: { block: TimelineBlock; width: number } = $props();
  /** A few stops read along a rail; more of them, or a narrow board, read down. */
  const across = $derived(block.stops.length <= 6 && width >= 600);
</script>

<ol class="timeline" class:across style:--stops={block.stops.length}>
  {#each block.stops as stop, index (index)}<li>
      <span class="when">{stop.when}</span>
      <span class="dot" aria-hidden="true"></span>
      <span class="title">{stop.title}</span>
      {#if stop.detail}<span class="detail">{stop.detail}</span>{/if}
    </li>{/each}
</ol>

<style>
  .timeline {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin: 0;
    padding: 0 0 0 20px;
    list-style: none;
  }

  .timeline::before {
    content: "";
    position: absolute;
    inset-block: 6px;
    inset-inline-start: 4px;
    inline-size: 1px;
    background: var(--color-border-strong);
  }

  li {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .dot {
    position: absolute;
    inset-block-start: 5px;
    inset-inline-start: -20px;
    inline-size: 9px;
    block-size: 9px;
    border-radius: var(--radius-capsule);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 2px var(--color-text);
  }

  .when {
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
    line-height: 16px;
  }

  .title {
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 18px;
  }

  .detail {
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    line-height: 17px;
  }

  .across {
    display: grid;
    grid-template-columns: repeat(var(--stops), minmax(0, 1fr));
    gap: 16px;
    padding: 0;
  }

  .across::before {
    inset-block: auto;
    inset-block-start: 26px;
    inset-inline: 4px;
    inline-size: auto;
    block-size: 1px;
  }

  .across li {
    padding-block-start: 0;
  }

  .across .when {
    block-size: 16px;
  }

  .across .dot {
    position: static;
    margin-block: 4px 8px;
  }
</style>

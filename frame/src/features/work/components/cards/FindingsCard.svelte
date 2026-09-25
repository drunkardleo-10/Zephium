<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { Tick02Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { FINDINGS_ROWS as ROWS } from "../../lib/card-size";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const items = $derived(item.findings?.items ?? []);
  const total = $derived(Math.max(item.findings?.total ?? 0, items.length));
</script>

<CardFrame
  id={item.id}
  title={m.work_card_findings()}
  count={total}
  icon={Tick02Icon}
  {selected}
  active={item.active}
  unavailable={item.unavailable}
  dense
>
  <div class="findings">
    <ul>
      {#each items.slice(0, ROWS) as finding, index (index)}
        <li class={finding.confidence}>
          <span class="dot" aria-hidden="true"></span>
          <span class="claim">{finding.claim}</span>
          {#if finding.subject}<span class="subject">{finding.subject}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if total > ROWS}<p class="more">{m.work_card_more({ count: total - ROWS })}</p>{/if}
  </div>
</CardFrame>

<style>
  .findings {
    display: flex;
    flex-direction: column;
    gap: 3px;
    block-size: 100%;
    min-block-size: 0;
    padding-block-end: 6px;
    box-sizing: border-box;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-block-size: 0;
    margin: 0;
    padding: 0;
    overflow: hidden;
    list-style: none;
  }

  li {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    flex: none;
    min-inline-size: 0;
  }

  .dot {
    flex: none;
    inline-size: 6px;
    block-size: 6px;
    margin-block-start: 5px;
    border-radius: 50%;
    background: var(--color-faint);
  }

  .supported .dot {
    background: var(--color-success);
  }

  .contradicted .dot {
    background: var(--color-danger);
  }

  .claim {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    flex: 1;
    min-inline-size: 0;
    overflow: hidden;
    font-size: var(--text-label);
    line-height: 16px;
    text-wrap: pretty;
  }

  .subject {
    flex: none;
    max-inline-size: 88px;
    padding: 0 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font-size: var(--text-caption);
    line-height: 16px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .more {
    flex: none;
    margin: 0;
    padding-inline-start: 14px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 13px;
  }
</style>

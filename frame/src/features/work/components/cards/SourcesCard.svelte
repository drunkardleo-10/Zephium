<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import HostGlyph from "./HostGlyph.svelte";
  import { Link04Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { SOURCE_ROWS } from "../../lib/card-size";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const MARKS = 8;
  const rows = $derived(item.sources ?? []);
  /** One mark per site: eleven pages of one shop are one shop. */
  const mark = (row: (typeof rows)[number]) => (row.file ? `file:${row.key}` : row.where);
  const marks = $derived(
    rows.filter((row, index) => rows.findIndex((other) => mark(other) === mark(row)) === index),
  );
</script>

<CardFrame title={item.title} icon={Link04Icon} {selected} active={item.active} dense>
  {#if marks.length}<div class="marks" aria-hidden="true">
      {#each marks.slice(0, MARKS) as row (row.key)}<HostGlyph
          host={row.where}
          file={!!row.file}
          size={18}
        />{/each}
      {#if marks.length > MARKS}<span class="extra">+{marks.length - MARKS}</span>{/if}
    </div>{/if}
  <ul class="rows">
    {#each rows.slice(0, SOURCE_ROWS) as row (row.key)}
      <li>
        <span class="where">{row.where}</span>
        <span class="title" class:refused={!!row.note} title={row.note || row.title}
          >{row.note || row.title}</span
        >
      </li>
    {/each}
  </ul>
</CardFrame>

<style>
  .marks {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-block-end: 10px;
  }

  .extra {
    padding: 0 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
    line-height: 18px;
  }

  .rows {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0 0 4px;
    overflow: hidden;
  }

  li {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-inline-size: 0;
  }

  .where {
    flex: none;
    max-inline-size: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 16px;
  }

  .title {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-label);
    line-height: 16px;
  }

  .title.refused {
    color: var(--color-muted);
  }
</style>

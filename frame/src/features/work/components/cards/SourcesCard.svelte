<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import HostGlyph from "./HostGlyph.svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { SOURCE_ROWS } from "../../lib/card-size";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const rows = $derived(item.sources ?? []);
</script>

<!-- The count heads the card; each row carries its own site's mark. -->
<CardFrame id={item.id} title={item.title} {selected} active={item.active} lines={1} dense>
  <ul class="rows">
    {#each rows.slice(0, SOURCE_ROWS) as row (row.key)}
      <li>
        <HostGlyph host={row.where} url={row.url} file={!!row.file} size={14} />
        <span class="where">{row.where}</span>
        <span class="title" class:refused={!!row.note} title={row.note || row.title}
          >{row.note || row.title}</span
        >
      </li>
    {/each}
  </ul>
</CardFrame>

<style>
  .rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0 -4px;
    padding: 0;
    list-style: none;
    overflow: hidden;
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
    min-block-size: 20px;
    padding-inline: 4px;
    border-radius: var(--radius-row);
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  li:hover {
    background: var(--row-hover);
  }

  .where {
    flex: none;
    max-inline-size: 40%;
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

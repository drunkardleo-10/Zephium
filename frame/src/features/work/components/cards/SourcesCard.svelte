<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import HostGlyph from "./HostGlyph.svelte";
  import { Link04Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const rows = $derived(item.sources ?? []);
</script>

<CardFrame
  kind={item.kind}
  title={item.title}
  icon={Link04Icon}
  {selected}
  active={item.active}
  dense
>
  <ul class="rows">
    {#each rows.slice(0, 6) as row (row.key)}
      <li>
        <HostGlyph host={row.where} file={!!row.file} />
        <span class="text">
          <span class="where">{row.where}</span>
          <span class="title" class:refused={!!row.note} title={row.title}
            >{row.note || row.title}</span
          >
        </span>
      </li>
    {/each}
  </ul>
</CardFrame>

<style>
  .rows {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    block-size: 100%;
    min-block-size: 0;
    margin: 0;
    padding: 0 0 4px;
    overflow: hidden;
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
  }

  .text {
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
  }

  .title {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-label);
  }

  .title.refused {
    color: var(--color-muted);
  }
</style>

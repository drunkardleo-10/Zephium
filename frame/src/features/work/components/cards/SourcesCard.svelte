<script lang="ts">
  import { getContext } from "svelte";
  import CardFrame from "./CardFrame.svelte";
  import HostGlyph from "./HostGlyph.svelte";
  import Icon from "$shared/ui/Icon";
  import { ArrowDown01Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { canvasBoard, type BoardActions } from "../../lib/canvas-context";
  import { SOURCE_ROWS } from "../../lib/card-size";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const rows = $derived(item.sources ?? []);
  const unread = $derived(item.unread ?? []);
  /** The pages that could not be read take the rows' place while they are open. */
  let open = $state(false);
  const actions = getContext<BoardActions | undefined>(canvasBoard);
  const frames = $derived(item.frames ?? []);
</script>

<!-- The count heads the card; each row carries its own site's mark. -->
<CardFrame
  id={item.id}
  title={item.title}
  {selected}
  active={item.active}
  lines={1}
  dense
  footer={unread.length ? more : undefined}
>
  {#if frames.length}<ul class="frames" aria-label={m.work_board_frames()}>
      {#each frames as page (page.key)}<li>
          <button
            type="button"
            class="frame nodrag nopan"
            title={page.url}
            onclick={(event) => {
              event.stopPropagation();
              actions?.page(page.url);
            }}
            ><img
              src={page.frame}
              alt={page.host}
              loading="lazy"
              decoding="async"
              draggable="false"
              width="64"
              height="40"
            /></button
          >
        </li>{/each}
    </ul>{/if}
  {#if open && unread.length}
    <ul class="rows unread" aria-label={m.work_env_unread_list()}>
      {#each unread as page (page.key)}
        <li title={page.url}>
          <HostGlyph host={page.host} url={page.url} size={14} />
          <span class="where">{page.host}</span>
          <span class="title refused">{page.note}</span>
        </li>
      {/each}
    </ul>
  {:else}
    <ul class="rows">
      {#each rows.slice(0, SOURCE_ROWS) as row (row.key)}
        <li>
          <HostGlyph host={row.where} url={row.url} file={!!row.file} size={14} />
          <span class="where">{row.where.replace(/^www\./u, "")}</span>
          <span class="title" class:refused={!!row.note} title={row.note || row.title}
            >{row.note || row.title}</span
          >
        </li>
      {/each}
    </ul>
  {/if}
</CardFrame>

{#snippet more()}<button
    type="button"
    class="more nodrag nopan"
    aria-expanded={open}
    onclick={(event) => {
      event.stopPropagation();
      open = !open;
    }}
    >{unread.length === 1
      ? m.work_env_unread_one()
      : m.work_env_unread_count({ count: unread.length })}<Icon
      icon={ArrowDown01Icon}
      size={11}
    /></button
  >{/snippet}

<style>
  .frames {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 6px;
    margin: 0 0 12px;
    padding: 0;
    list-style: none;
  }

  .frame {
    display: block;
    inline-size: 100%;
    aspect-ratio: 16 / 10;
    overflow: hidden;
    padding: 0;
    border: 0;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px var(--color-border);
    cursor: default;
  }

  .frame img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    object-position: top;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0 -4px;
    padding: 0;
    list-style: none;
    overflow: hidden;
  }

  /* Past the rows' height they scroll, so every unread page stays in the card. */
  .rows.unread {
    block-size: 100%;
    overflow-y: auto;
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

  /* One quiet line: it discloses, it does not report a result. */
  .more {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-inline-size: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-faint);
    font: inherit;
    cursor: default;
    transition: color var(--motion-fast) var(--ease-out);
  }

  .more:hover {
    color: var(--color-muted);
  }

  .more:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .more :global(svg) {
    flex: none;
    transition: rotate var(--motion-base) var(--ease-emphasized);
  }

  .more[aria-expanded="true"] :global(svg) {
    rotate: 180deg;
  }
</style>

<script lang="ts">
  import { getContext, untrack } from "svelte";
  import Icon from "$shared/ui/Icon";
  import HostGlyph from "./HostGlyph.svelte";
  import { ArrowDown01Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { canvasBoard, type BoardActions } from "../../lib/canvas-context";
  import { SOURCE_ROWS } from "../../lib/project-environment-board";
  import type { RunSource } from "../../lib/run/sources";
  import * as m from "$shared/i18n/messages";

  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const board = getContext<BoardActions | undefined>(canvasBoard);
  const rows = $derived(item.drawn?.rows ?? []);
  const unread = $derived(item.drawn?.unread ?? []);
  /** Pictures of the pages the run read, up to four, one site each before a site's second. */
  const frames = $derived.by(() => {
    const framed = rows.filter((row) => row.frame);
    const first = framed.filter(
      (row, index) => framed.findIndex((other) => other.host === row.host) === index,
    );
    return [...first, ...framed.filter((row) => !first.includes(row))].slice(0, 4);
  });
  let all = $state(false);
  let missed = $state(false);
  const shown = $derived(all ? rows : rows.slice(0, SOURCE_ROWS));

  function open(row: RunSource) {
    if (row.file)
      board?.evidence({ key: row.key, label: row.title, origin: row.host, file: row.file });
    else if (row.url) board?.page(row.url);
  }

  // Its own height, so the run makes the room it takes as it opens.
  let body = $state<HTMLElement>();
  $effect(() => {
    const element = body;
    const width = item.size?.width ?? 0;
    if (!element || !width) return;
    let reported = 0;
    const report = () => {
      const height = Math.ceil(element.offsetHeight);
      if (!height || height === reported) return;
      reported = height;
      untrack(() => board?.measure(item.id, width, false, height));
    };
    report();
    const observer = new ResizeObserver(() => requestAnimationFrame(report));
    observer.observe(element);
    return () => observer.disconnect();
  });
</script>

<!-- What the run drew on: pictures of what it read, then a row per page, then what it could not open. -->
<section
  class="sources work-drag-handle"
  class:selected
  aria-label={m.work_sources()}
  data-card-id={item.id}
>
  <div bind:this={body}>
    <h3>{m.work_sources()}</h3>
    {#if frames.length}<ul class="frames">
        {#each frames as row (row.key)}<li>
            <button
              type="button"
              class="frame nodrag nopan"
              title={row.title}
              aria-label={row.title}
              onclick={(event) => {
                event.stopPropagation();
                open(row);
              }}
              ><img
                src={row.frame}
                alt=""
                loading="lazy"
                decoding="async"
                draggable="false"
              /></button
            >
          </li>{/each}
      </ul>{/if}
    {#if rows.length}<ul class="rows">
        {#each shown as row (row.key)}<li>
            <button
              type="button"
              class="row nodrag nopan"
              title={row.url || row.file?.path}
              onclick={(event) => {
                event.stopPropagation();
                open(row);
              }}
            >
              <span class="mark"
                ><HostGlyph host={row.host} url={row.url} file={!!row.file} size={16} /></span
              >
              <span class="site">{row.host}</span>
              <span class="title">{row.title}</span>
            </button>
          </li>{/each}
      </ul>{/if}
    {#if rows.length > SOURCE_ROWS}<button
        type="button"
        class="quiet nodrag nopan"
        aria-expanded={all}
        onclick={(event) => {
          event.stopPropagation();
          all = !all;
        }}
        >{all ? m.work_sources_fewer() : m.work_sources_all({ count: rows.length })}<Icon
          icon={ArrowDown01Icon}
          size={11}
        /></button
      >{/if}
    {#if unread.length}
      <button
        type="button"
        class="quiet missed nodrag nopan"
        aria-expanded={missed}
        onclick={(event) => {
          event.stopPropagation();
          missed = !missed;
        }}
        >{unread.length === 1
          ? m.work_sources_unread_one()
          : m.work_sources_unread({ count: unread.length })}<Icon
          icon={ArrowDown01Icon}
          size={11}
        /></button
      >
      {#if missed}<ul class="rows unread" aria-label={m.work_env_unread_list()}>
          {#each unread as page (page.key)}<li>
              <button
                type="button"
                class="row nodrag nopan"
                title={page.url}
                onclick={(event) => {
                  event.stopPropagation();
                  board?.page(page.url);
                }}
              >
                <span class="mark"><HostGlyph host={page.host} url={page.url} size={16} /></span>
                <span class="site">{page.host}</span>
                <span class="title">{page.note}</span>
              </button>
            </li>{/each}
        </ul>{/if}
    {/if}
  </div>
</section>

<style>
  .sources {
    box-sizing: border-box;
    inline-size: 100%;
    color: var(--color-text);
  }

  .sources.selected {
    border-radius: var(--radius-row);
    box-shadow: 0 0 0 1.5px var(--color-ring);
  }

  h3 {
    margin: 0 0 8px;
    color: var(--color-muted);
    font-size: var(--text-label);
    font-weight: 600;
    line-height: 20px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .frames {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
    margin-block-end: 12px;
  }

  .frame {
    display: block;
    inline-size: 100%;
    aspect-ratio: 16 / 10;
    overflow: hidden;
    padding: 0;
    border: 0;
    border-radius: var(--radius-inset);
    background: var(--color-surface);
    box-shadow:
      0 0 0 1px var(--color-border),
      var(--shadow-raised);
    cursor: default;
    transition: translate var(--motion-base) var(--ease-spring);
  }

  .frame:hover {
    translate: 0 -2px;
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
    margin-inline: -8px;
  }

  .row {
    display: grid;
    grid-template-columns: 16px 112px minmax(0, 1fr);
    align-items: center;
    gap: 10px;
    inline-size: 100%;
    block-size: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .row:hover {
    background: var(--color-control-hover);
  }

  .mark {
    display: grid;
    place-items: center;
    inline-size: 16px;
    block-size: 16px;
  }

  .site {
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    white-space: nowrap;
    mask-image: linear-gradient(to right, black calc(100% - 16px), transparent);
  }

  /* One line each: a long title fades at the column's edge rather than breaking off with dots. */
  .title {
    overflow: hidden;
    font-size: var(--text-body);
    white-space: nowrap;
    mask-image: linear-gradient(to right, black calc(100% - 24px), transparent);
  }

  .unread .title {
    color: var(--color-muted);
  }

  .quiet {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-block-start: 6px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    line-height: 20px;
    cursor: default;
    transition: color var(--motion-fast) var(--ease-out);
  }

  .quiet:hover {
    color: var(--color-text);
  }

  .missed {
    display: flex;
    color: var(--color-faint);
  }

  .quiet :global(svg) {
    flex: none;
    transition: rotate var(--motion-base) var(--ease-emphasized);
  }

  .quiet[aria-expanded="true"] :global(svg) {
    rotate: 180deg;
  }

  .frame:focus-visible,
  .row:focus-visible,
  .quiet:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>

<script lang="ts">
  import DataTable, {
    type TableColumn,
    type TableRow,
    type TableSort,
  } from "$shared/ui/data/DataTable";
  import * as m from "$shared/i18n/messages";
  import type { ObjectActions, SheetView } from "../../lib/board/types";
  import { bests, cellOrder, figure, fitColumns, host, numeric, rating, yesNo } from "./sheet";
  import { vendorHost } from "../../lib/vendors";
  import Title from "./Title.svelte";
  import Mark, { hasMark } from "./Mark.svelte";
  import YesNo from "./YesNo.svelte";
  import Dots from "./Dots.svelte";
  let {
    object,
    actions = {},
    rows: shown = 8,
    centre = false,
  }: {
    object: SheetView;
    actions?: ObjectActions;
    /** Rows the canvas shows before the rest are read in the centre. */
    rows?: number;
    /** Opened in the centre: every row, every column. */
    centre?: boolean;
  } = $props();
  const first = $derived(object.columns[0]);
  const best = $derived(bests(object));
  let width = $state(0);
  /** Every column once the person asks for them; they scroll inside the sheet. */
  let wide = $state(false);
  /** The columns that carry most in the width the sheet stands at. */
  const fitted = $derived(
    width ? fitColumns(object, width - 44) : object.columns.map((_, index) => index).slice(1),
  );
  const kept = $derived(centre || wide ? object.columns.map((_, index) => index).slice(1) : fitted);
  const hidden = $derived(object.columns.length - 1 - fitted.length);
  const columns = $derived<TableColumn[]>(
    kept.map((at) => {
      const column = object.columns[at]!;
      return {
        key: String(at),
        label:
          column.unit && column.kind !== "money"
            ? `${column.label} (${column.unit})`
            : column.label,
        numeric: numeric(column),
        centered: column.kind === "yes_no" || column.kind === "rating",
        sortable: object.rows.length > 2 && column.kind !== "link",
      };
    }),
  );
  const rows = $derived<TableRow[]>(
    object.rows.map((row, index) => ({
      key: String(index),
      label: row.cells[0] ?? "",
      cells: Object.fromEntries(row.cells.map((cell, at) => [String(at), cell])),
      sort: Object.fromEntries(
        object.columns.map((column, at) => [String(at), cellOrder(column, row.cells[at] ?? "")]),
      ),
    })),
  );
  let sort = $state<TableSort | null>(null);
  const labels = $derived({
    rowHeading: first?.label ?? "",
    actions: m.work_actions(),
    empty: m.work_empty_data(),
    missing: "",
    unavailable: m.work_artifact_unavailable(),
    previous: m.work_previous(),
    next: m.work_next(),
    range: (from: number, to: number, total: number) =>
      m.work_table_range({ first: from, last: to, total }),
  });
  /** The row's own logo, else a known product its name is. */
  function logo(index: number): string | null {
    const row = object.rows[index];
    const own = row?.entity?.logo;
    if (own && hasMark(own)) return own;
    if (first?.kind !== "entity") return null;
    const known = vendorHost(undefined, row?.cells[0] ?? "");
    return known && hasMark(known) ? known : null;
  }
  const limit = $derived(centre ? object.rows.length : shown);
  /** Where the columns run on past the sheet's edge, each side. */
  let more = $state({ before: false, after: false });
  let table = $state<HTMLElement>();
  function measure(scroller: Element | null | undefined) {
    if (!(scroller instanceof HTMLElement)) return;
    const end = scroller.scrollWidth - scroller.clientWidth;
    more = { before: scroller.scrollLeft > 1, after: end - scroller.scrollLeft > 1 };
  }
  $effect(() => {
    void kept;
    void width;
    const scroller = table?.querySelector(".table-scroll");
    const frame = requestAnimationFrame(() => measure(scroller));
    return () => cancelAnimationFrame(frame);
  });
</script>

{#snippet subject(index: number, size: number)}
  {@const row = object.rows[index]}
  {#if row?.entity?.picture}<img
      class="picture"
      src={row.entity.picture.src}
      alt=""
      decoding="async"
      loading="lazy"
      width={size + 8}
      height={size + 8}
    />{:else if logo(index)}<Mark address={logo(index)!} {size} />{/if}
{/snippet}

{#snippet value(index: number, at: number)}
  {@const column = object.columns[at]!}
  {@const text = object.rows[index]?.cells[at] ?? ""}
  {@const marked = best[at]?.has(String(index))}
  {#if column.kind === "yes_no"}<YesNo value={yesNo(text)} size={16} />
  {:else if column.kind === "rating"}
    {@const score = rating(text)}
    {#if score}<Dots
        value={score.value}
        max={score.max}
        label={m.work_object_rating({ value: score.value, max: score.max })}
      />{:else}<span class="text">{text}</span>{/if}
  {:else if column.kind === "entity"}
    {@const known = vendorHost(undefined, text)}
    <span class="subject"
      >{#if known}<Mark address={known} size={16} />{/if}<span class="name">{text}</span></span
    >
  {:else if column.kind === "tag"}{#if text.trim()}<span class="tag">{text}</span>{/if}
  {:else if column.kind === "link"}
    <a
      class="link nodrag"
      href={text}
      onclick={(event) => {
        if (!actions.link) return;
        event.preventDefault();
        actions.link(text);
      }}><Mark address={text} size={14} /><span>{host(text)}</span></a
    >
  {:else if numeric(column)}<span class="figure" class:best={marked}>{figure(column, text)}</span>
  {:else}<span class="text">{text}</span>{/if}
{/snippet}

<section class="sheet" class:centre aria-label={object.title} bind:clientWidth={width}>
  {#if object.title}<Title text={object.title} />{/if}
  <div
    class="nodrag table"
    class:before={more.before}
    class:after={more.after}
    class:nowheel={more.before || more.after}
    bind:this={table}
    onscrollcapture={(event) => measure(event.target as Element)}
  >
    <DataTable
      caption={object.title ?? ""}
      showCaption={false}
      {columns}
      {rows}
      {labels}
      plain
      headSortable={object.rows.length > 2}
      bind:sort
      {limit}
    >
      {#snippet head(row)}
        <span class="subject"
          >{@render subject(Number(row.key), 18)}<span class="name">{row.label}</span></span
        >
      {/snippet}
      {#snippet cell(row, column)}{@render value(Number(row.key), Number(column.key))}{/snippet}
    </DataTable>
  </div>
  {#if object.rows.length > limit || object.note || (hidden > 0 && !centre)}
    <footer>
      {#if object.note}<p class="note">{object.note}</p>{/if}
      <span class="more">
        {#if hidden > 0 && !centre}<button
            type="button"
            class="all nodrag nopan"
            aria-expanded={wide}
            onclick={() => (wide = !wide)}
            >{wide
              ? m.work_sheet_fewer_columns()
              : hidden === 1
                ? m.work_sheet_more_column()
                : m.work_sheet_more_columns({ count: hidden })}</button
          >{/if}
        {#if object.rows.length > limit}<button
            type="button"
            class="all nodrag nopan"
            onclick={() => actions.open?.(object.id)}
            >{m.work_object_show_all({ count: object.rows.length })}</button
          >{/if}
      </span>
    </footer>
  {/if}
</section>

<style>
  .sheet {
    display: flex;
    flex-direction: column;
    gap: 14px;
    box-sizing: border-box;
    inline-size: 100%;
    padding: 20px 22px 16px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
    font-size: var(--text-body);
  }

  .subject {
    display: inline-flex;
    align-items: center;
    gap: 10px;
  }

  /* A name wraps between its words, never inside one. */
  .name {
    overflow-wrap: normal;
    text-wrap: pretty;
  }

  .picture {
    flex: none;
    border-radius: var(--radius-inset);
    object-fit: cover;
  }

  .text {
    display: block;
    min-inline-size: 12ch;
    max-inline-size: 34ch;
    color: var(--color-label-secondary);
    overflow-wrap: break-word;
    text-wrap: pretty;
  }

  /* Columns that run on past the edge fade into it; the subject column stays. */
  .table.after :global(.table-scroll) {
    mask-image: linear-gradient(to left, transparent, black 48px);
  }

  .table.before :global(tbody th),
  .table.before :global(thead th:first-child) {
    box-shadow: 1px 0 0 var(--color-border);
  }

  .table :global(thead th:first-child) {
    position: sticky;
    inset-inline-start: 0;
    z-index: 1;
    background: var(--color-surface);
  }

  .figure {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .figure.best {
    padding: 2px 8px;
    margin-inline-end: -8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-active);
    color: var(--color-text);
    font-weight: 650;
  }

  .tag {
    display: inline-block;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    white-space: nowrap;
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--color-label-secondary);
    text-decoration: none;
  }

  .link:hover span {
    color: var(--color-text);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  footer {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 16px;
  }

  .more {
    display: flex;
    flex: none;
    gap: 16px;
    margin-inline-start: auto;
  }

  .note {
    margin: 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .all {
    flex: none;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
  }

  .all:hover {
    color: var(--color-text);
  }
</style>

<script lang="ts">
  import DataTable, {
    type TableColumn,
    type TableRow,
    type TableSort,
  } from "$shared/ui/data/DataTable";
  import * as m from "$shared/i18n/messages";
  import type { Detail, ObjectActions, SheetView } from "../../lib/board/types";
  import { bests, cellOrder, figure, host, numeric, rating, yesNo } from "./sheet";
  import { vendorHost } from "../../lib/vendors";
  import Title from "./Title.svelte";
  import Mark, { hasMark } from "./Mark.svelte";
  import YesNo from "./YesNo.svelte";
  import Dots from "./Dots.svelte";
  let {
    object,
    detail,
    actions = {},
    rows: shown = 8,
    centre = false,
  }: {
    object: SheetView;
    detail: Detail;
    actions?: ObjectActions;
    /** Rows the canvas shows before the rest are read in the centre. */
    rows?: number;
    /** Opened in the centre: every row, every word. */
    centre?: boolean;
  } = $props();
  const first = $derived(object.columns[0]);
  const rest = $derived(object.columns.slice(1));
  const best = $derived(bests(object));
  const columns = $derived<TableColumn[]>(
    rest.map((column, index) => ({
      key: String(index + 1),
      label:
        column.unit && column.kind !== "money" ? `${column.label} (${column.unit})` : column.label,
      numeric: numeric(column),
      centered: column.kind === "yes_no" || column.kind === "rating",
      sortable: object.rows.length > 2 && column.kind !== "link",
    })),
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
  /** At a distance a sheet is its subjects and the one column that decides between them. */
  const key = $derived.by(() => {
    const marked = object.columns.findIndex((column, index) => index > 0 && column.best);
    if (marked > 0) return marked;
    const figureColumn = object.columns.findIndex((column, index) => index > 0 && numeric(column));
    if (figureColumn > 0) return figureColumn;
    return object.columns.findIndex((column, index) => index > 0 && column.kind === "entity");
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
  const limit = $derived(centre ? object.rows.length : detail === "full" ? shown : 5);
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
  {#if column.kind === "yes_no"}<YesNo value={yesNo(text)} />
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
      >{#if known}<Mark address={known} size={detail === "full" ? 16 : 28} />{/if}<span class="name"
        >{text}</span
      ></span
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
  {:else if numeric(column)}<span class="figure" class:best={marked}
      >{figure(column, text)}{#if column.kind === "number" && column.unit && detail !== "full"}<span
          class="unit">{column.unit}</span
        >{/if}</span
    >
  {:else}<span class="text">{text}</span>{/if}
{/snippet}

<section class="sheet {detail}" class:centre aria-label={object.title}>
  {#if object.title}<Title text={object.title} {detail} />{/if}
  {#if detail === "full"}
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
    {#if object.rows.length > limit || object.note}
      <footer>
        {#if object.note}<p class="note">{object.note}</p>{/if}
        {#if object.rows.length > limit}<button
            type="button"
            class="all nodrag nopan"
            onclick={() => actions.open?.(object.id)}
            >{m.work_object_show_all({ count: object.rows.length })}</button
          >{/if}
      </footer>
    {/if}
  {:else if detail === "overview"}
    <ol class="far">
      {#each object.rows.slice(0, limit) as row, index (index)}
        <li>
          <span class="subject"
            >{@render subject(index, 28)}<span class="name">{row.cells[0]}</span></span
          >
          {#if key > 0}<span class="key">{@render value(index, key)}</span>{/if}
        </li>
      {/each}
    </ol>
  {:else}
    <div class="marks">
      {#each object.rows.slice(0, 6) as row, index (index)}
        {#if row.entity?.picture || logo(index)}{@render subject(index, 56)}{/if}
      {/each}
    </div>
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

  .sheet.overview {
    gap: 22px;
    padding: 28px;
  }

  .sheet.tile {
    gap: 24px;
    padding: 32px;
  }

  .subject {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    min-inline-size: 0;
  }

  .name {
    min-inline-size: 0;
    overflow-wrap: anywhere;
  }

  .picture {
    flex: none;
    border-radius: var(--radius-inset);
    object-fit: cover;
  }

  .text {
    display: -webkit-box;
    min-inline-size: 16ch;
    overflow: hidden;
    color: var(--color-label-secondary);
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    text-wrap: pretty;
  }

  .centre .text {
    display: inline;
    -webkit-line-clamp: none;
    line-clamp: none;
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

  .unit {
    margin-inline-start: 0.25em;
    color: var(--color-muted);
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

  .note {
    margin: 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .all {
    flex: none;
    margin-inline-start: auto;
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

  .far {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--text-overview-label);
  }

  .far li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding-block: 12px;
    border-block-start: 2px solid var(--color-border);
  }

  .far .name {
    font-weight: 600;
  }

  .far .key :global(.mark) {
    inline-size: 28px;
    block-size: 28px;
  }

  .far .figure.best {
    padding: 2px 14px;
    margin-inline-end: 0;
  }

  .marks {
    display: flex;
    flex-wrap: wrap;
    gap: 20px;
  }

  .marks .picture {
    border-radius: var(--radius-row);
  }
</style>

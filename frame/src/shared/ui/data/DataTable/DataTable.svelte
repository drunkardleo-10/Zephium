<script lang="ts">
  import type { Snippet } from "svelte";
  import {
    sortRows,
    tableInputValid,
    type TableColumn,
    type TableRow,
    type TableLabels,
    type TableSort,
  } from "./table";
  let {
    caption,
    columns,
    rows,
    labels,
    pageSize = 25,
    showCaption = true,
    actions,
    cell,
    head,
    headSortable = false,
    sort = $bindable(null),
    plain = false,
    limit,
  }: {
    caption: string;
    columns: readonly TableColumn[];
    rows: readonly TableRow[];
    labels: TableLabels;
    pageSize?: number;
    showCaption?: boolean;
    actions?: Snippet<[TableRow]>;
    /** Draws a cell's value; plain text otherwise. */
    cell?: Snippet<[TableRow, TableColumn]>;
    /** Draws a row's heading; its label otherwise. */
    head?: Snippet<[TableRow]>;
    /** The heading column sorts by row label. */
    headSortable?: boolean;
    sort?: TableSort | null;
    /** No frame of its own: the owner's surface holds it, its heading column stays put. */
    plain?: boolean;
    /** Shows the first rows only, with no pages: the rest are read elsewhere. */
    limit?: number;
  } = $props();
  let page = $state(0);
  let size = $derived(
    Number.isFinite(pageSize) ? Math.max(1, Math.min(100, Math.floor(pageSize))) : 25,
  );
  let valid = $derived(tableInputValid(rows, columns));
  let pages = $derived(Math.max(1, Math.ceil(rows.length / size)));
  let current = $derived(Math.min(page, pages - 1));
  let first = $derived(current * size);
  let ordered = $derived(sortRows(rows, sort));
  let visible = $derived(
    !valid
      ? []
      : limit !== undefined
        ? ordered.slice(0, Math.max(0, limit))
        : ordered.slice(first, first + size),
  );
  function toggle(key: string) {
    sort =
      sort?.key !== key
        ? { key, descending: false }
        : sort.descending
          ? null
          : { key, descending: true };
  }
  const order = (key: string) =>
    sort?.key === key ? (sort.descending ? "descending" : "ascending") : undefined;
</script>

{#if !valid}<p role="alert">{labels.unavailable}</p>
{:else}
  {#snippet heading(key: string, label: string, sortable: boolean)}
    {#if sortable}<button
        type="button"
        class="sort"
        class:on={sort?.key === key}
        onclick={() => toggle(key)}
        >{label}<svg viewBox="0 0 8 8" aria-hidden="true" class:down={sort?.descending}
          ><path d="M1.5 5 4 2.5 6.5 5" /></svg
        ></button
      >{:else}{label}{/if}
  {/snippet}
  <!-- The scroll region needs keyboard focus for horizontal navigation. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="table-scroll" class:plain role="region" aria-label={caption} tabindex="0">
    <table>
      <caption class:sr-only={!showCaption}>{caption}</caption>
      <thead
        ><tr
          ><th scope="col" aria-sort={order("")}
            >{@render heading("", labels.rowHeading, headSortable)}</th
          >{#each columns as column (column.key)}<th
              scope="col"
              class:numeric={column.numeric}
              class:centered={column.centered}
              aria-sort={order(column.key)}
              >{@render heading(column.key, column.label, !!column.sortable)}</th
            >{/each}{#if actions}<th scope="col"><span class="sr-only">{labels.actions}</span></th
            >{/if}</tr
        ></thead
      >
      <tbody>
        {#each visible as row (row.key)}
          <tr data-row-key={row.key}>
            <th scope="row"
              >{#if head}{@render head(row)}{:else}{row.label}{/if}</th
            >
            {#each columns as column (column.key)}<td
                class:numeric={column.numeric}
                class:centered={column.centered}
                >{#if cell}{@render cell(row, column)}{:else}{row.cells[column.key] ??
                    labels.missing}{/if}</td
              >{/each}
            {#if actions}<td>{@render actions(row)}</td>{/if}
          </tr>
        {:else}<tr><td colspan={columns.length + 1 + (actions ? 1 : 0)}>{labels.empty}</td></tr
          >{/each}
      </tbody>
    </table>
  </div>
  {#if pages > 1 && limit === undefined}
    <div class="pagination">
      <span role="status">{labels.range(first + 1, first + visible.length, rows.length)}</span>
      <div>
        <button type="button" disabled={current === 0} onclick={() => (page = current - 1)}
          >{labels.previous}</button
        ><button type="button" disabled={current === pages - 1} onclick={() => (page = current + 1)}
          >{labels.next}</button
        >
      </div>
    </div>
  {/if}
{/if}

<style>
  .table-scroll {
    max-width: 100%;
    overflow: auto;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-card);
    background: var(--color-surface);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font: inherit;
  }

  caption {
    padding: 16px;
    text-align: start;
    font-weight: 600;
    color: var(--color-text);
  }

  th,
  td {
    min-inline-size: 160px;
    padding: 10px 14px;
    border-block-start: 1px solid var(--color-border);
    text-align: start;
    vertical-align: top;
    overflow-wrap: anywhere;
  }

  thead {
    background: var(--color-fill);
    color: var(--color-muted);
  }

  th {
    font-weight: 500;
  }

  td {
    color: var(--color-text);
  }

  .numeric {
    text-align: end;
    font-variant-numeric: tabular-nums;
  }

  .centered {
    text-align: center;
  }

  .sort {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    cursor: default;
  }

  .sort:hover,
  .sort.on {
    color: var(--color-text);
  }

  .sort svg {
    inline-size: 8px;
    block-size: 8px;
    fill: none;
    stroke: currentcolor;
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0;
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .sort.on svg {
    opacity: 1;
  }

  .sort svg.down {
    transform: rotate(180deg);
  }

  .numeric .sort {
    flex-direction: row-reverse;
  }

  /* On its owner's surface: no frame, rows parted by hairlines, the heading column held. */
  .plain table {
    border-collapse: separate;
    border-spacing: 0;
  }

  .plain {
    border: 0;
    border-radius: 0;
    background: transparent;
  }

  /* A word is never broken: a column is at least as wide as its longest word. */
  .plain th,
  .plain td {
    min-inline-size: 0;
    padding: 11px 12px;
    vertical-align: middle;
    overflow-wrap: break-word;
  }

  .plain thead {
    background: none;
    font-size: var(--text-label);
  }

  /* A heading wraps between its words, so a narrow column's name never widens it. */
  .plain thead th {
    padding-block: 0 9px;
    border-block-start: 0;
    vertical-align: bottom;
    overflow-wrap: normal;
    text-wrap: balance;
  }

  .plain thead .sort {
    text-align: inherit;
  }

  .plain thead .centered .sort {
    justify-content: center;
  }

  .plain tbody th {
    position: sticky;
    inset-inline-start: 0;
    z-index: 1;
    background: var(--color-surface);
    color: var(--color-text);
    font-weight: 600;
  }

  .plain th:first-child,
  .plain td:first-child {
    padding-inline-start: 0;
  }

  .plain th:last-child,
  .plain td:last-child {
    padding-inline-end: 0;
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px;
    padding-block: 12px;
    color: var(--color-muted);
  }

  .pagination > div {
    display: flex;
    gap: 8px;
  }

  button {
    padding: 6px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-row);
    background: var(--color-fill);
    color: var(--color-text);
    font: inherit;
  }

  button:disabled {
    opacity: 0.5;
  }

  button:focus-visible,
  .table-scroll:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>

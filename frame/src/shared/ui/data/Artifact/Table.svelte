<script lang="ts">
  import { bestCells, sortedRows, tableColumns, tableCsv, type TableSort } from "./table";
  import * as m from "$shared/i18n/messages";
  let {
    caption,
    columns,
    rows,
  }: {
    caption: string;
    columns: readonly string[];
    rows: readonly (readonly string[])[];
  } = $props();
  const grid = $derived({ columns: [...columns], rows: rows.map((row) => [...row]) });
  const shape = $derived(tableColumns(grid));
  const best = $derived(bestCells(grid));
  let sort = $state<TableSort | null>(null);
  const order = $derived(sortedRows(grid, sort));
  /** Ascending, then descending, then the order the run wrote. */
  function toggle(column: number) {
    sort =
      sort?.column !== column
        ? { column, descending: false }
        : sort.descending
          ? null
          : { column, descending: true };
  }
  const direction = (column: number) =>
    sort?.column === column ? (sort.descending ? "descending" : "ascending") : "none";
  let copied = $state<"idle" | "copied" | "failed">("idle");
  function copy() {
    const text = tableCsv(
      columns,
      order.map((index) => rows[index]!),
    );
    void navigator.clipboard.writeText(text).then(
      () => (copied = "copied"),
      () => (copied = "failed"),
    );
  }
</script>

<!-- The whole table: its header and first column stay put while the rest scrolls. -->
<div class="table">
  <div class="scroll" role="region" aria-label={caption}>
    <table>
      <caption>{caption}</caption>
      <colgroup>
        {#each columns as _, index (index)}<col
            class:long={index === shape.long && columns.length > 2}
          />{/each}
      </colgroup>
      <thead>
        <tr>
          {#each columns as column, index (index)}<th
              scope="col"
              class:figure={shape.numeric[index]}
              aria-sort={direction(index)}
              ><button
                type="button"
                class="sort"
                title={m.work_table_sort({ column: column || m.work_table_row() })}
                onclick={() => toggle(index)}
                ><span class="label">{column}</span><span
                  class="arrow"
                  class:on={sort?.column === index}
                  class:down={sort?.column === index && sort.descending}
                  aria-hidden="true"
                ></span></button
              ></th
            >{/each}
        </tr>
      </thead>
      <tbody>
        {#each order as index (index)}{@const row = rows[index]!}<tr>
            {#each row as cell, column (column)}{@const marked = best
                .get(column)
                ?.includes(index)}{#if column === 0}<th scope="row">{cell}</th>{:else}<td
                  class:figure={shape.numeric[column]}
                  >{#if marked}<span
                      class="best"
                      role="img"
                      title={m.work_compare_best()}
                      aria-label={m.work_compare_best()}
                    ></span>{/if}{cell}</td
                >{/if}{/each}
          </tr>{/each}
      </tbody>
    </table>
  </div>
  <footer>
    <span class="count"
      >{rows.length === 1
        ? m.work_table_count_one()
        : m.work_table_count({ count: rows.length })}</span
    >
    {#if copied === "failed"}<span class="failed" role="alert">{m.work_code_copy_failed()}</span
      >{/if}
    <button type="button" class="copy" onclick={copy}
      >{copied === "copied" ? m.work_code_copied() : m.work_table_copy_csv()}</button
    >
  </footer>
</div>

<style>
  .table {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-inline-size: 0;
  }

  .scroll {
    max-block-size: min(70vh, 640px);
    overflow: auto;
    border-radius: var(--radius-row);
    box-shadow: inset 0 0 0 1px var(--color-border);
  }

  table {
    inline-size: 100%;
    border-collapse: separate;
    border-spacing: 0;
    font-size: var(--text-body);
    line-height: 19px;
    font-variant-numeric: tabular-nums;
  }

  caption {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  col.long {
    inline-size: 40%;
  }

  th,
  td {
    min-inline-size: 96px;
    padding: 9px 12px;
    border-block-end: 1px solid var(--color-border);
    background: var(--color-surface);
    text-align: start;
    vertical-align: top;
    overflow-wrap: anywhere;
  }

  thead th {
    position: sticky;
    inset-block-start: 0;
    z-index: 1;
    padding: 0;
    border-block-end-color: var(--color-border-strong);
    color: var(--color-muted);
    font-size: var(--text-label);
    font-weight: 500;
    white-space: nowrap;
  }

  .sort {
    display: flex;
    align-items: center;
    gap: 6px;
    inline-size: 100%;
    padding: 8px 12px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: inherit;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .sort:hover {
    background: var(--row-hover);
  }

  .sort:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .figure .sort {
    flex-direction: row-reverse;
  }

  .sort:hover,
  th[aria-sort="ascending"] .sort,
  th[aria-sort="descending"] .sort {
    color: var(--color-text);
  }

  .label {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* A small chevron, drawn only for the sorted column or under the pointer. */
  .arrow {
    flex: none;
    inline-size: 5px;
    block-size: 5px;
    margin-block-start: -3px;
    border-inline-end: 1.5px solid currentcolor;
    border-block-end: 1.5px solid currentcolor;
    opacity: 0;
    rotate: 225deg;
    translate: 0 3px;
    transition:
      opacity var(--motion-fast) var(--ease-out),
      rotate var(--motion-base) var(--ease-spring);
  }

  .sort:hover .arrow {
    opacity: 0.45;
  }

  .arrow.on {
    opacity: 1;
  }

  .arrow.down {
    rotate: 45deg;
    translate: 0 0;
  }

  tbody th {
    position: sticky;
    inset-inline-start: 0;
    font-weight: 600;
  }

  thead th:first-child {
    inset-inline-start: 0;
    z-index: 2;
  }

  tbody tr:last-child > * {
    border-block-end: 0;
  }

  .figure {
    text-align: end;
    white-space: nowrap;
  }

  .best {
    display: inline-block;
    inline-size: 6px;
    block-size: 6px;
    margin-inline-end: 6px;
    border-radius: 50%;
    background: var(--color-accent);
    vertical-align: 0.1em;
  }

  footer {
    display: flex;
    align-items: center;
    gap: 12px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .count {
    flex: 1;
    font-variant-numeric: tabular-nums;
  }

  .failed {
    color: var(--color-danger);
  }

  .copy {
    padding: 2px 10px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-control);
    color: var(--color-text);
    font: inherit;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .copy:hover {
    background: var(--color-control-hover);
  }

  .copy:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>

<script lang="ts">
  import { sortedRows, tableCsv, type TableSort } from "$shared/ui/data/Artifact/table";
  import { TABLE_ROWS } from "../../lib/board/size";
  import type { BoardActions } from "../../lib/canvas-context";
  import type { ColumnType, TableBlock } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let {
    block,
    actions,
    open = false,
    within = false,
  }: {
    block: TableBlock;
    actions?: BoardActions;
    open?: boolean;
    /** Drawn inside another block, which opens and closes it. */
    within?: boolean;
  } = $props();
  const grid = $derived({
    columns: block.columns.map((column) => column.label),
    rows: block.rows.map((row) => [...row]),
  });
  let sort = $state<TableSort | null>(null);
  const order = $derived(sortedRows(grid, sort));
  const shown = $derived(open ? order : order.slice(0, TABLE_ROWS));
  const rest = $derived(block.rows.length - TABLE_ROWS);
  const WEIGHT: Record<ColumnType, number> = {
    text: 3,
    long: 6,
    number: 2,
    money: 2.4,
    date: 2.6,
    link: 3.4,
  };
  const total = $derived(block.columns.reduce((sum, column) => sum + WEIGHT[column.type], 0));
  const figure = (type: ColumnType) => type === "number" || type === "money";
  function toggle(column: number) {
    sort =
      sort?.column !== column
        ? { column, descending: figure(block.columns[column]!.type) }
        : sort.descending === figure(block.columns[column]!.type)
          ? { column, descending: !sort.descending }
          : null;
  }
  const direction = (column: number) =>
    sort?.column === column ? (sort.descending ? "descending" : "ascending") : "none";
  const host = (url: string) => {
    try {
      return new URL(url).host.replace(/^www\./u, "");
    } catch {
      return url;
    }
  };
  let copied = $state(false);
  function copy() {
    void navigator.clipboard
      .writeText(
        tableCsv(
          grid.columns,
          order.map((index) => grid.rows[index]!),
        ),
      )
      .then(() => (copied = true));
  }
</script>

<div class="table" class:open>
  <div class="scroll nowheel" role="region" aria-label={block.title ?? ""}>
    <table>
      <colgroup>
        {#each block.columns as column, index (index)}<col
            style:inline-size={`${(WEIGHT[column.type] / total) * 100}%`}
          />{/each}
      </colgroup>
      <thead>
        <tr>
          {#each block.columns as column, index (index)}<th
              scope="col"
              class:figure={figure(column.type)}
              aria-sort={direction(index)}
              ><button
                type="button"
                class="sort nodrag nopan"
                title={m.work_table_sort({ column: column.label || m.work_table_row() })}
                onclick={() => toggle(index)}
                ><span class="label">{column.label}</span><span
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
        {#each shown as index (index)}{@const row = block.rows[index]!}<tr>
            {#each row as cell, column (column)}{@const type =
                block.columns[column]?.type ?? "text"}<td
                class:figure={figure(type)}
                class:first={column === 0}
                >{#if type === "link" && cell}<button
                    type="button"
                    class="link nodrag nopan"
                    title={cell}
                    onclick={() => actions?.page(cell)}>{host(cell)}</button
                  >{:else if type === "long"}<span class:clamp={!open}>{cell}</span
                  >{:else}{cell}{/if}</td
              >{/each}
          </tr>{/each}
      </tbody>
    </table>
  </div>
  {#if rest > 0 || open}<footer>
      {#if rest > 0 && !within}<button
          type="button"
          class="more nodrag nopan"
          onclick={() => actions?.toggle(block.id)}
          >{open
            ? m.work_board_fewer()
            : m.work_board_all_rows({ count: block.rows.length })}</button
        >{/if}
      {#if open}<button type="button" class="more nodrag nopan" onclick={copy}
          >{copied ? m.work_code_copied() : m.work_table_copy_csv()}</button
        >{/if}
    </footer>{/if}
</div>

<style>
  .table {
    min-inline-size: 0;
    font-size: var(--text-body);
  }

  .scroll {
    overflow-x: auto;
  }

  .open .scroll {
    max-block-size: 640px;
    overflow-y: auto;
  }

  table {
    inline-size: 100%;
    border-collapse: collapse;
    table-layout: fixed;
  }

  thead th {
    position: sticky;
    inset-block-start: 0;
    z-index: 1;
    padding: 0;
    border-block-end: 1px solid var(--color-border-strong);
    background: var(--color-surface);
    text-align: start;
  }

  th.figure {
    text-align: end;
  }

  .sort {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-inline-size: 100%;
    block-size: 30px;
    padding: 0 8px;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
  }

  th.figure .sort {
    flex-direction: row-reverse;
  }

  th .sort:hover {
    color: var(--color-text);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .arrow {
    flex: none;
    inline-size: 0;
    block-size: 0;
    border-inline: 3.5px solid transparent;
    border-block-end: 4px solid currentcolor;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--ease-out),
      rotate var(--motion-fast) var(--ease-out);
  }

  .sort:hover .arrow {
    opacity: 0.4;
  }

  .arrow.on {
    opacity: 1;
  }

  .arrow.down {
    rotate: 180deg;
  }

  td {
    padding: 8px;
    border-block-end: 1px solid var(--color-border);
    color: var(--color-label-secondary);
    line-height: 19px;
    vertical-align: top;
    overflow-wrap: anywhere;
  }

  td.first {
    color: var(--color-text);
    font-weight: 500;
  }

  td.figure {
    color: var(--color-text);
    font-variant-numeric: tabular-nums;
    text-align: end;
    white-space: nowrap;
  }

  .clamp {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
  }

  tbody tr:last-child td {
    border-block-end: 0;
  }

  tbody tr {
    transition: background-color var(--motion-instant) var(--ease-out);
  }

  tbody tr:hover {
    background: var(--row-hover);
  }

  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-decoration: underline;
    text-decoration-color: var(--color-border-strong);
    text-underline-offset: 3px;
    cursor: default;
  }

  footer {
    display: flex;
    gap: 6px;
    margin-block-start: 12px;
  }

  .more {
    block-size: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .more:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>

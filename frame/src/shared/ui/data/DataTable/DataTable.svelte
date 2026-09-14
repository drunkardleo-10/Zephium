<script lang="ts">
  import type { Snippet } from "svelte";
  import { tableInputValid, type TableColumn, type TableRow, type TableLabels } from "./table";
  let {
    caption,
    columns,
    rows,
    labels,
    pageSize = 25,
    showCaption = true,
    actions,
  }: {
    caption: string;
    columns: readonly TableColumn[];
    rows: readonly TableRow[];
    labels: TableLabels;
    pageSize?: number;
    showCaption?: boolean;
    actions?: Snippet<[TableRow]>;
  } = $props();
  let page = $state(0);
  let size = $derived(
    Number.isFinite(pageSize) ? Math.max(1, Math.min(100, Math.floor(pageSize))) : 25,
  );
  let valid = $derived(tableInputValid(rows, columns));
  let pages = $derived(Math.max(1, Math.ceil(rows.length / size)));
  let current = $derived(Math.min(page, pages - 1));
  let first = $derived(current * size);
  let visible = $derived(valid ? rows.slice(first, first + size) : []);
</script>

{#if !valid}<p role="alert">{labels.unavailable}</p>
{:else}
  <!-- The scroll region needs keyboard focus for horizontal navigation. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="table-scroll" role="region" aria-label={caption} tabindex="0">
    <table>
      <caption class:sr-only={!showCaption}>{caption}</caption>
      <thead
        ><tr
          ><th scope="col">{labels.rowHeading}</th>{#each columns as column (column.key)}<th
              scope="col"
              class:numeric={column.numeric}>{column.label}</th
            >{/each}{#if actions}<th scope="col"><span class="sr-only">{labels.actions}</span></th
            >{/if}</tr
        ></thead
      >
      <tbody>
        {#each visible as row (row.key)}
          <tr data-row-key={row.key}>
            <th scope="row">{row.label}</th>
            {#each columns as column (column.key)}<td class:numeric={column.numeric}
                >{row.cells[column.key] ?? labels.missing}</td
              >{/each}
            {#if actions}<td>{@render actions(row)}</td>{/if}
          </tr>
        {:else}<tr><td colspan={columns.length + 1 + (actions ? 1 : 0)}>{labels.empty}</td></tr
          >{/each}
      </tbody>
    </table>
  </div>
  {#if pages > 1}
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
    border-radius: var(--radius-lg);
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
    border-radius: var(--radius-sm);
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

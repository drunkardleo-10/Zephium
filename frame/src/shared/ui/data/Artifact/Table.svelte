<script lang="ts">
  import { tableColumns } from "./table";
  let {
    caption,
    columns,
    rows,
  }: {
    caption: string;
    columns: readonly string[];
    rows: readonly (readonly string[])[];
  } = $props();
  const shape = $derived(
    tableColumns({ columns: [...columns], rows: rows.map((row) => [...row]) }),
  );
</script>

<!-- The whole table: its header and first column stay put while the rest scrolls. -->
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
        {#each columns as column, index (index)}<th scope="col" class:figure={shape.numeric[index]}
            >{column}</th
          >{/each}
      </tr>
    </thead>
    <tbody>
      {#each rows as row, index (index)}<tr>
          {#each row as cell, column (column)}{#if column === 0}<th scope="row">{cell}</th
              >{:else}<td class:figure={shape.numeric[column]}>{cell}</td>{/if}{/each}
        </tr>{/each}
    </tbody>
  </table>
</div>

<style>
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
    font-size: var(--text-label);
    line-height: 16px;
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
    padding: 8px 12px;
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
    color: var(--color-muted);
    font-weight: 500;
    white-space: nowrap;
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
</style>

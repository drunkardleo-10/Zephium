<script lang="ts">
  import type { ArtifactContent } from "$shared/ui/data/Artifact";
  import { TABLE_CARD, tableColumns, tableGrid } from "$shared/ui/data/Artifact/table";
  let { content }: { content: Extract<ArtifactContent, { kind: "table" | "comparison" }> } =
    $props();
  const grid = $derived(tableGrid(content));
  const shape = $derived(tableColumns(grid));
  const columns = $derived(grid.columns.slice(0, TABLE_CARD.columns));
  const rows = $derived(
    grid.rows.slice(0, TABLE_CARD.rows).map((row) => row.slice(0, TABLE_CARD.columns)),
  );
</script>

<!-- A real mini table: the header, six rows, four columns; the lift has the rest. -->
<table class="mini">
  <thead>
    <tr>
      {#each columns as column, index (index)}<th scope="col" class:figure={shape.numeric[index]}
          ><span>{column}</span></th
        >{/each}
    </tr>
  </thead>
  <tbody>
    {#each rows as row, index (index)}<tr>
        {#each row as cell, column (column)}<td class:figure={shape.numeric[column]}
            ><span>{cell}</span></td
          >{/each}
      </tr>{/each}
  </tbody>
</table>

<style>
  .mini {
    inline-size: 100%;
    table-layout: fixed;
    border-collapse: collapse;
    font-size: var(--text-label);
    line-height: 16px;
    font-variant-numeric: tabular-nums;
  }

  th,
  td {
    padding: 4px 6px;
    border-block-end: 1px solid var(--color-border);
    text-align: start;
    vertical-align: top;
  }

  th {
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 500;
  }

  th span {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  td span {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    overflow-wrap: anywhere;
  }

  th:first-child,
  td:first-child {
    padding-inline-start: 0;
  }

  td:first-child {
    font-weight: 500;
  }

  th:last-child,
  td:last-child {
    padding-inline-end: 0;
  }

  tbody tr:last-child td {
    border-block-end: 0;
  }

  .figure {
    text-align: end;
  }
</style>

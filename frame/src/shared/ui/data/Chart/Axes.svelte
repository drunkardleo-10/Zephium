<script lang="ts">
  import type { Geometry } from "./geometry";
  /** Hairline gridlines and rounded ticks; no axis lines. */
  let { shape }: { shape: Geometry } = $props();
  const plot = $derived(shape.plot);
</script>

{#each shape.ticks as tick, index (index)}
  {#if shape.valueAxis === "y"}
    <line
      class="grid"
      class:zero={tick.zero}
      x1={plot.left}
      x2={plot.right}
      y1={tick.pos}
      y2={tick.pos}
    />
    {#if tick.label}
      <text class="tick" x={plot.left - 6} y={tick.pos + 3} text-anchor="end">{tick.label}</text>
    {/if}
  {:else}
    <line
      class="grid"
      class:zero={tick.zero}
      x1={tick.pos}
      x2={tick.pos}
      y1={plot.top}
      y2={plot.bottom}
    />
    {#if tick.label}
      <text class="tick" x={tick.pos} y={plot.bottom + 14} text-anchor="middle">{tick.label}</text>
    {/if}
  {/if}
{/each}
{#each shape.labels as label, index (index)}
  <text class="tick" x={label.pos} y={plot.bottom + 16} text-anchor="middle">{label.text}</text>
{/each}
{#each shape.rows as row, index (index)}
  <text class="tick row" x={plot.left - 8} y={row.pos + 3.5} text-anchor="end">{row.text}</text>
{/each}

<style>
  .grid {
    stroke: var(--color-border);
    stroke-width: 1;
    shape-rendering: crispedges;
  }

  .grid.zero {
    stroke: var(--color-border-strong);
  }

  .tick {
    fill: var(--color-faint);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .row {
    fill: var(--color-muted);
  }
</style>

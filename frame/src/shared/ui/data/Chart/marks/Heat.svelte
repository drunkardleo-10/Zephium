<script lang="ts">
  import type { Cell } from "../geometry";
  /** One hue from the surface rung to the lit rung; an empty cell is only the fill. */
  let {
    cells,
    hovered = null,
    enter = false,
  }: { cells: readonly Cell[]; hovered?: number | null; enter?: boolean } = $props();
  const tint = (tone: number | null) =>
    tone === null
      ? "var(--color-fill)"
      : `color-mix(in oklab, var(--color-lit) ${Math.round(6 + tone * 94)}%, var(--color-surface))`;
</script>

{#each cells as cell (cell.index)}
  <rect
    class="cell"
    class:enter
    x={cell.x}
    y={cell.y}
    width={cell.width}
    height={cell.height}
    style:fill={tint(cell.tone)}
  />
{/each}
{#each cells.filter((cell) => cell.index === hovered) as cell (cell.index)}
  <rect class="ring" x={cell.x} y={cell.y} width={cell.width} height={cell.height} />
{/each}

<style>
  .ring {
    fill: none;
    stroke: var(--color-text);
    stroke-width: 1.5;
    pointer-events: none;
    animation: arrive var(--motion-fast) var(--ease-smooth) both;
  }

  .cell.enter {
    animation: arrive var(--motion-slow) var(--ease-emphasized) both;
  }

  @keyframes arrive {
    from {
      opacity: 0;
    }
  }
</style>

<script lang="ts">
  import ObjectView from "../components/objects/ObjectView.svelte";
  import type { ObjectView as View } from "../lib/board/types";
  /** Each object at full (100%), overview (as seen at 50%) and tile (as seen at 30%). */
  let {
    rows,
    levels = ["full", "overview", "tile"],
  }: {
    rows: readonly { object: View; width: number }[];
    levels?: readonly ("full" | "overview" | "tile")[];
  } = $props();
  const ZOOM = { full: 1, overview: 0.5, tile: 0.3 } as const;
</script>

<div class="sheet">
  {#each rows as row (row.object.id)}
    <div class="row" data-id={row.object.id}>
      {#each levels as level (level)}
        <div class="cell" style:zoom={ZOOM[level]} style:inline-size={`${row.width}px`}>
          <ObjectView object={row.object} detail={level} />
        </div>
      {/each}
    </div>
  {/each}
</div>

<style>
  .sheet {
    display: flex;
    flex-direction: column;
    gap: 0;
    inline-size: max-content;
    background: var(--color-canvas);
    color: var(--color-text);
    font-family: var(--font-sans);
  }

  .row {
    display: flex;
    padding: 24px;
    background: var(--color-canvas);
    align-items: flex-start;
    gap: 48px;
  }

  .cell {
    flex: none;
  }
</style>

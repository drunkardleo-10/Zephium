<script lang="ts">
  import ObjectView from "../components/objects/ObjectView.svelte";
  import ObjectCentre from "../components/objects/ObjectCentre.svelte";
  import type { ObjectActions, ObjectView as View } from "../lib/board/types";
  /** Each object at full (100%), overview (as seen at 50%) and tile (as seen at 30%). */
  let {
    rows,
    actions = {},
    centre = false,
    levels = ["full", "overview", "tile"],
  }: {
    rows: readonly { object: View; width: number }[];
    actions?: ObjectActions;
    /** Each object as it opens in the centre, on the lift's surface. */
    centre?: boolean;
    levels?: readonly ("full" | "overview" | "tile")[];
  } = $props();
  const ZOOM = { full: 1, overview: 0.5, tile: 0.3 } as const;
</script>

<div class="sheet">
  {#each rows as row (row.object.id)}
    <div class="row" class:stacked={row.width > 900} data-id={row.object.id}>
      {#if centre}<div class="well" style:inline-size={`${row.width}px`}>
          <ObjectCentre object={row.object} {actions} />
        </div>{:else}{#each levels as level (level)}
          <div class="cell" style:zoom={ZOOM[level]} style:inline-size={`${row.width}px`}>
            <ObjectView object={row.object} detail={level} {actions} />
          </div>
        {/each}{/if}
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

  .row.stacked {
    flex-direction: column;
  }

  .well {
    box-sizing: border-box;
    padding: 32px 40px;
    border-radius: var(--radius-panel);
    background: var(--color-surface);
    box-shadow: var(--shadow-overlay);
  }

  .cell {
    flex: none;
  }
</style>

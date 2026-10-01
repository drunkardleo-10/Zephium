<script lang="ts">
  import ObjectView from "../components/objects/ObjectView.svelte";
  import ObjectCentre from "../components/objects/ObjectCentre.svelte";
  import type { ObjectActions, ObjectView as View } from "../lib/board/types";
  /** Each object as it stands at 100%, and the same object as the canvas shows it at 50%. */
  let {
    rows,
    actions = {},
    centre = false,
    zooms = [1, 0.5],
  }: {
    rows: readonly { object: View; width: number }[];
    actions?: ObjectActions;
    /** Each object as it opens in the centre, on the lift's surface. */
    centre?: boolean;
    zooms?: readonly number[];
  } = $props();
  let heights = $state<Record<string, number>>({});
</script>

<div class="sheet">
  {#each rows as row (row.object.id)}
    <div class="row" class:stacked={row.width > 1600} data-id={row.object.id}>
      {#if centre}<div class="well" style:inline-size={`${row.width}px`}>
          <ObjectCentre object={row.object} {actions} />
        </div>{:else}{#each zooms as zoom (zoom)}
          <!-- Scaled as the canvas scales it: the same layout, drawn smaller. -->
          <div
            class="cell"
            style:inline-size={`${row.width * zoom}px`}
            style:block-size={`${(heights[`${row.object.id}:${zoom}`] ?? 0) * zoom}px`}
          >
            <div
              class="scaled"
              style:inline-size={`${row.width}px`}
              style:transform={zoom === 1 ? undefined : `scale(${zoom})`}
              bind:clientHeight={heights[`${row.object.id}:${zoom}`]}
            >
              <ObjectView object={row.object} {actions} />
            </div>
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
    position: relative;
    flex: none;
  }

  .scaled {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    transform-origin: 0 0;
  }
</style>

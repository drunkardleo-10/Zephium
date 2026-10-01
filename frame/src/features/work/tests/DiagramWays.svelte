<script lang="ts">
  import BoardDiagram from "../components/board/Diagram.svelte";
  import type { DiagramWay } from "../lib/diagram";
  import type { DiagramView } from "../lib/board/types";
  /** Each diagram drawn one way at its own full size, never scaled to a result's width. */
  let {
    diagrams,
    way,
    zoom = 1,
  }: { diagrams: readonly DiagramView[]; way: DiagramWay; zoom?: number } = $props();
</script>

<div class="sheet">
  {#each diagrams as diagram (diagram.id)}
    <div class="row" data-id={diagram.id} style:zoom>
      <p class="title">{diagram.title}</p>
      <BoardDiagram diagram={diagram.diagram} width={4000} {way} />
    </div>
  {/each}
</div>

<style>
  .sheet {
    display: flex;
    flex-direction: column;
    inline-size: max-content;
    background: var(--color-canvas);
    color: var(--color-text);
    font-family: var(--font-sans);
  }

  .row {
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 32px;
    background: var(--color-canvas);
  }

  .title {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
  }
</style>

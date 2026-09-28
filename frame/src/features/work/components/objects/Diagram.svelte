<script lang="ts">
  import type { Detail, DiagramView, ObjectActions } from "../../lib/board/types";
  import BoardDiagram from "../board/Diagram.svelte";
  import Title from "./Title.svelte";
  /** Parts and their relations, set on the canvas: the tiers are alignment, never boxes. */
  let {
    object,
    detail,
    actions = {},
  }: { object: DiagramView; detail: Detail; actions?: ObjectActions } = $props();
  let width = $state(0);
</script>

<section class="diagram {detail}" aria-label={object.title} bind:clientWidth={width}>
  {#if object.title}<Title text={object.title} {detail} />{/if}
  {#if width}<BoardDiagram diagram={object.diagram} {width} {detail} ask={actions.ask} />{/if}
</section>

<style>
  .diagram {
    display: flex;
    flex-direction: column;
    gap: 20px;
    inline-size: 100%;
  }
</style>

<script lang="ts">
  import type { Detail, ObjectActions, ObjectView } from "../../lib/board/types";
  import Plot from "./Plot.svelte";
  import Sheet from "./Sheet.svelte";
  import Reply from "./Reply.svelte";
  import Picks from "./Picks.svelte";
  import Plan from "./Plan.svelte";
  import List from "./List.svelte";
  import Diagram from "./Diagram.svelte";
  /** One canvas object at the detail its on-screen size allows. */
  let {
    object,
    detail = "full",
    actions = {},
  }: { object: ObjectView; detail?: Detail; actions?: ObjectActions } = $props();
</script>

{#if object.kind === "plot"}<Plot {object} {detail} />
{:else if object.kind === "sheet"}<Sheet {object} {detail} {actions} />
{:else if object.kind === "reply"}<Reply {object} {detail} />
{:else if object.kind === "picks"}<Picks {object} {detail} {actions} />
{:else if object.kind === "plan"}<Plan {object} {detail} {actions} />
{:else if object.kind === "list"}<List {object} {detail} {actions} />
{:else if object.kind === "diagram"}<Diagram {object} {detail} {actions} />{/if}

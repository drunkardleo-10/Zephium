<script lang="ts">
  import type { Detail, ObjectView } from "../../lib/board/types";
  import Plot from "./Plot.svelte";
  /** One canvas object at the detail its on-screen size allows. */
  let { object, detail = "full" }: { object: ObjectView; detail?: Detail } = $props();
</script>

{#if object.kind === "plot"}<Plot {object} {detail} />{/if}

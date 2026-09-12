<script lang="ts">
  import RenderBoundary from "../RenderBoundary.svelte";
  import Throws from "./Throws.svelte";
  let { fail }: { fail: boolean } = $props();
</script>

<RenderBoundary title="Cannot display this view" retryLabel="Retry"
  ><Throws {fail} /></RenderBoundary
>

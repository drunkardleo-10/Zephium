<script lang="ts">
  import type { Component } from "svelte";
  import LazyView from "../LazyView.svelte";
  let { loader }: { loader: () => Promise<{ default: Component<{ label: string }> }> } = $props();
</script>

<LazyView {loader} loadingLabel="Loading view" failureLabel="View unavailable" retryLabel="Retry">
  {#snippet children(View)}<View label="Ready" />{/snippet}
</LazyView>

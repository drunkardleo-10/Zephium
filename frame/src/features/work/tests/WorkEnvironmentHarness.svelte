<script lang="ts">
  import WorkEnvironment from "../components/WorkEnvironment.svelte";
  let {
    onreturn,
    onpanelchange,
  }: { onreturn: () => void; onpanelchange: (panel: string | null) => void } = $props();
</script>

{#snippet tabs()}<input aria-label="Find a tab" />{/snippet}
{#snippet profile()}<button type="button">Work settings</button>{/snippet}
{#snippet children()}<button type="button">Canvas object</button>{/snippet}
<div class="host">
  <WorkEnvironment
    spaceName="Personal"
    workTitle="Research"
    profileLabel="Reader"
    initialTabsOpen
    panels={{ tabs, profile }}
    {children}
    {onreturn}
    {onpanelchange}
  />
</div>

<style>
  .host {
    inline-size: 1000px;
    block-size: 700px;
  }
</style>

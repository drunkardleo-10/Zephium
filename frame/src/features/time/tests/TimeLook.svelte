<script lang="ts">
  import "$styles/global.css";
  import FocusCover from "../components/FocusCover.svelte";
  import TimePage from "../components/TimePage.svelte";
  import TimePanel from "../components/TimePanel.svelte";

  let {
    profile,
    view,
    span = "day",
  }: { profile: string; view: "panel" | "page"; span?: "day" | "week" } = $props();
</script>

{#if view === "panel"}
  <aside class="browser-sidebar" style="inline-size: 336px; block-size: 100%; overflow: auto">
    <TimePanel {profile} {span} onopen={() => {}} />
  </aside>
{:else if view === "cover"}
  <div style="block-size: 100%; padding: 8px">
    <FocusCover site="youtube.com" />
  </div>
{:else}
  <div style="block-size: 100%; padding: 8px">
    <TimePage />
  </div>
{/if}

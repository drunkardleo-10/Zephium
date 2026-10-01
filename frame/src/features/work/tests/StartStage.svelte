<script lang="ts">
  import WorkBar from "../components/bar/WorkBar.svelte";
  import WorkStart from "../components/start/WorkStart.svelte";
  import type { RecentWork } from "../lib/start/workflows";

  let { works = [], value = $bindable("") }: { works?: readonly RecentWork[]; value?: string } =
    $props();
  let bar = $state<HTMLElement>();
  let height = $state(0);
  let skill = $state<string | null>(null);
</script>

<!-- A new work as the Work screen holds it: the empty canvas, the start screen over it, the ask bar on its bottom edge. -->
<div class="stage" data-skill={skill ?? ""}>
  <div class="canvas">
    <WorkStart
      bind:value
      onskill={(next) => (skill = next)}
      field={bar}
      {works}
      profile="p"
      inset={height}
    />
    <div class="dock" bind:clientHeight={height}>
      <WorkBar bind:ref={bar} bind:value placeholder="Ask for anything" onsubmit={() => {}} />
    </div>
  </div>
</div>

<style>
  .stage {
    position: fixed;
    inset: 0;
    padding: 8px 8px 0;
    background: var(--color-chrome);
  }

  .canvas {
    position: relative;
    block-size: 100%;
    overflow: hidden;
    border-radius: var(--content-radius) var(--content-radius) 0 0;
    background: var(--color-canvas);
  }

  .dock {
    position: absolute;
    inset: auto 0 0;
    display: flex;
    justify-content: center;
  }
</style>

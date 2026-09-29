<script lang="ts">
  // A live run on the real canvas with the island over it, as the Work screen stacks them.
  import type { WorkSession } from "$domain/work";
  import AgentLine from "../components/AgentLine.svelte";
  import PartsCanvas from "./PartsCanvas.svelte";
  import type { BoardScene } from "./board-fixtures";
  let {
    scene,
    session,
    working,
    viewport,
  }: {
    scene: BoardScene;
    session: WorkSession;
    working: {
      id: string;
      title: string;
      now: string;
      host?: string;
      helper?: "browser" | "research" | "computer" | "connection";
    }[];
    viewport: { x: number; y: number; zoom: number };
  } = $props();
</script>

<div class="stage">
  <PartsCanvas {scene} {viewport} />
  <div class="island"><AgentLine {session} {working} /></div>
</div>

<style>
  .stage {
    position: relative;
    inline-size: 100%;
    block-size: 100%;
  }

  .island {
    position: absolute;
    inset-block-start: 12px;
    inset-inline-start: 50%;
    z-index: 10;
    display: flex;
    justify-content: center;
    inline-size: min(560px, 60%);
    translate: -50% 0;
  }
</style>

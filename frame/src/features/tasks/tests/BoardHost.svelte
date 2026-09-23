<script lang="ts">
  import { untrack } from "svelte";
  import { taskSession } from "$domain/resources";
  import TaskBoard from "../components/TaskBoard.svelte";

  let { profile, host = "board" }: { profile: string; host?: string } = $props();

  let session = $state.raw(untrack(() => taskSession(profile, host)));
  $effect(() => {
    const owner = profile;
    const surface = host;
    return untrack(() => {
      const current = taskSession(owner, surface);
      session = current;
      void current.start();
      return () => current.stop();
    });
  });
</script>

<div class="board-host">
  <TaskBoard
    rows={session.rows}
    onmove={(id, status, key) => session.move(id, status, key)}
    onselect={() => {}}
    onopenpage={() => {}}
  />
</div>

<style>
  .board-host {
    display: flex;
    width: 900px;
    height: 520px;
    padding: 12px;
    box-sizing: border-box;
    font: 13px system-ui;
    color: var(--color-text);
    background: var(--color-chrome);
  }
</style>

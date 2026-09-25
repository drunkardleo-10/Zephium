<script lang="ts">
  import { untrack } from "svelte";
  import { taskSession } from "$domain/resources";
  import Tasks from "../components/Tasks.svelte";
  import type { TaskScope } from "../lib/task-sections";

  let {
    profile,
    host = "sidebar",
    scope = "today",
    density = "panel",
    query = "",
  }: {
    profile: string;
    host?: string;
    scope?: TaskScope;
    density?: "panel" | "rail";
    query?: string;
  } = $props();

  let session = $state.raw(untrack(() => taskSession(profile, host)));
  $effect(() => {
    const owner = profile;
    const surface = host;
    return untrack(() => {
      const current = taskSession(owner, surface);
      session = current;
      void current.start(query);
      return () => current.stop();
    });
  });
</script>

<div class="task-host" style:width={density === "rail" ? "240px" : "336px"}>
  <Tasks {session} {scope} {density} {query} />
</div>

<style>
  .task-host {
    display: flex;
    flex-direction: column;
    height: 640px;
    padding: 12px;
    box-sizing: border-box;
    overflow: hidden;
    font: 13px system-ui;
    color: var(--color-text);
    background: var(--color-chrome);
  }
</style>

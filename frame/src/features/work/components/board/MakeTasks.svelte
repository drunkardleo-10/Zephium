<script lang="ts">
  import { getContext } from "svelte";
  import { workTasksKey, type WorkTasks } from "../../lib/work-tasks";
  import * as m from "$shared/i18n/messages";
  let { id }: { id: string } = $props();
  const tasks = getContext<WorkTasks | undefined>(workTasksKey);
  const state = $derived(tasks?.state(id) ?? "none");
</script>

{#if state !== "none"}<button
    type="button"
    class="make nodrag nopan"
    disabled={state !== "ready"}
    title={m.work_make_tasks_hint()}
    onclick={() => void tasks?.make(id)}
    >{state === "made"
      ? m.work_tasks_made()
      : state === "making"
        ? m.work_making_tasks()
        : m.work_make_tasks()}</button
  >{/if}

<style>
  .make {
    block-size: 26px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-lit);
    color: var(--color-on-lit);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 600;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .make:disabled {
    background: var(--color-fill);
    color: var(--color-muted);
  }

  .make:hover:not(:disabled) {
    background: var(--color-lit-hover);
  }
</style>

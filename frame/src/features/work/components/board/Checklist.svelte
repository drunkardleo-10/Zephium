<script lang="ts">
  import { getContext } from "svelte";
  import Icon from "$shared/ui/Icon";
  import MakeTasks from "./MakeTasks.svelte";
  import { Tick02Icon } from "../../lib/icons";
  import { workTasksKey, type WorkTasks } from "../../lib/work-tasks";
  import type { ChecklistBlock } from "../../lib/board/types";
  let { block }: { block: ChecklistBlock } = $props();
  const tasks = getContext<WorkTasks | undefined>(workTasksKey);
  const state = $derived(tasks?.state(block.id) ?? "none");
  const made = $derived(state === "none" ? [] : (tasks?.tasks(block.id) ?? []));
</script>

<ol class="list">
  {#each block.items as item, index (index)}{@const task = made[index]}
    <li class:done={item.completed || !!task?.completedAt}>
      <span class="mark" aria-hidden="true"
        >{#if item.completed || task?.completedAt}<Icon icon={Tick02Icon} size={11} />{/if}</span
      >
      {#if task}<button
          type="button"
          class="text task nodrag nopan"
          onclick={() => tasks?.open(task.id)}>{item.text}</button
        >{:else}<span class="text">{item.text}</span>{/if}
    </li>{/each}
</ol>
{#if state !== "none"}<footer><MakeTasks id={block.id} /></footer>{/if}

<style>
  .list {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 6px 0;
    border-block-end: 1px solid var(--color-border);
    font-size: var(--text-body);
    line-height: 19px;
  }

  li:last-child {
    border-block-end: 0;
  }

  .mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 16px;
    block-size: 16px;
    margin-block-start: 1.5px;
    border-radius: var(--radius-capsule);
    box-shadow: inset 0 0 0 1.25px var(--color-border-strong);
    color: var(--color-on-lit);
  }

  .text {
    min-inline-size: 0;
    color: var(--color-text);
  }

  .done .mark {
    background: var(--color-lit);
    box-shadow: none;
  }

  .done .text {
    color: var(--color-muted);
    text-decoration: line-through;
    text-decoration-color: var(--color-faint);
  }

  .task {
    padding: 0;
    border: 0;
    background: transparent;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .task:hover {
    text-decoration: underline;
    text-decoration-color: var(--color-border-strong);
    text-underline-offset: 3px;
  }

  footer {
    margin-block-start: 12px;
  }
</style>

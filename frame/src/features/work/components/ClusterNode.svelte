<script lang="ts">
  import { Handle, Position, type NodeProps, type Node } from "@xyflow/svelte";
  import { getContext, onMount } from "svelte";
  import { canvasArrival } from "../lib/canvas-context";
  import { arrive } from "../lib/arrival";
  import { stepsGroupResult, workTasksKey, type WorkTasks } from "../lib/work-tasks";
  import * as m from "$shared/i18n/messages";
  import type { Duration } from "$shared/lib/motion";
  import type { ClusterData } from "../lib/canvas-model";
  let { id, data }: NodeProps<Node<ClusterData, "cluster">> = $props();
  const arrival = getContext<((id: string) => Duration | null) | undefined>(canvasArrival);
  let root = $state<HTMLDivElement>();
  // A plan's steps group carries the one way its steps become the person's tasks.
  const tasks = getContext<WorkTasks | undefined>(workTasksKey);
  const plan = $derived(stepsGroupResult(id));
  const made = $derived(plan && tasks ? tasks.state(plan) : "none");
  // A group the run just made rises into place; one that was already there does not.
  onMount(() => {
    const motion = arrival?.(id);
    if (motion && root?.parentElement) arrive(root.parentElement, motion);
  });
</script>

<Handle
  type="target"
  position={Position.Left}
  isConnectable={false}
  tabindex={-1}
  aria-hidden="true"
/>
<div class="cluster" class:active={data.active} style:--inset="{data.inset}px" bind:this={root}>
  <span class="label"
    >{data.label}{#if data.more}<span class="more">+{data.more}</span
      >{/if}{#if plan && made !== "none"}<button
        type="button"
        class="make nodrag nopan"
        disabled={made !== "ready"}
        title={m.work_make_tasks_hint()}
        onclick={() => void tasks?.make(plan)}
        >{made === "made"
          ? m.work_tasks_made()
          : made === "making"
            ? m.work_making_tasks()
            : m.work_make_tasks()}</button
      >{/if}</span
  >
</div>
<Handle
  type="source"
  position={Position.Right}
  isConnectable={false}
  tabindex={-1}
  aria-hidden="true"
/>

<style>
  .cluster {
    position: relative;
    box-sizing: border-box;
    block-size: 100%;
    border-radius: var(--radius-panel);
    background: var(--row-hover);
    box-shadow: inset 0 0 0 1px var(--color-border);
    transition: box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .cluster.active {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .label {
    position: absolute;
    inset-block-start: var(--inset);
    inset-inline-start: var(--inset);
    display: flex;
    align-items: center;
    gap: 6px;
    block-size: 20px;
    padding-block-end: 4px;
    box-sizing: border-box;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .make {
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-control);
    color: var(--color-text);
    font: inherit;
    line-height: 16px;
    pointer-events: auto;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .make:disabled {
    background: transparent;
    color: var(--color-faint);
  }

  .make:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .make:hover:not(:disabled) {
    background: var(--color-control-hover);
  }

  .more {
    padding: 0 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
  }
</style>

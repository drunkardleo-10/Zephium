<script lang="ts">
  import {
    Handle,
    Position,
    useUpdateNodeInternals,
    type NodeProps,
    type Node,
  } from "@xyflow/svelte";
  import { getContext, onMount } from "svelte";
  import { canvasArrival, canvasOpen, canvasSelectGroup } from "../lib/canvas-context";
  import { arrive } from "../lib/arrival";
  import { stepsGroupResult, workTasksKey, type WorkTasks } from "../lib/work-tasks";
  import * as m from "$shared/i18n/messages";
  import type { Duration } from "$shared/lib/motion";
  import type { ClusterData } from "../lib/canvas-model";
  let { id, data }: NodeProps<Node<ClusterData, "cluster">> = $props();
  const arrival = getContext<((id: string) => Duration | null) | undefined>(canvasArrival);
  const selectGroup = getContext<((id: string) => void) | undefined>(canvasSelectGroup);
  const open = getContext<((id: string) => void) | undefined>(canvasOpen);
  // A tall group's lines meet its first row; the handles move with it.
  const updateInternals = useUpdateNodeInternals();
  const anchor = $derived(data.anchor === undefined ? undefined : `top: ${data.anchor}px`);
  $effect(() => {
    void data.anchor;
    updateInternals(id);
  });
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
  style={anchor}
/>
<div
  class="cluster"
  class:active={data.active}
  class:area={data.tone === "area"}
  style:--inset="{data.inset}px"
  bind:this={root}
>
  {#each data.layers ?? [] as layer (layer.name)}<span
      class="layer"
      style:transform="translate({layer.x}px, {layer.y - 16}px)">{layer.name}</span
    >{/each}
  <span class="label" class:headed={!!data.title}
    >{#if data.title}<button
        type="button"
        class="title nodrag nopan"
        title={data.title}
        onclick={() => data.opens && open?.(data.opens)}>{data.title}</button
      >{/if}{#if data.tone === "area"}<button
        type="button"
        class="count take nodrag nopan"
        title={m.work_diagram_select()}
        onclick={() => selectGroup?.(id)}>{data.label}</button
      >{:else if data.title}<span class="count">{data.label}</span
      >{:else}{data.label}{/if}{#if data.more}<span class="more">+{data.more}</span
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
  style={anchor}
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

  /* A diagram's area: the area's plate, its caption the one handle that takes it whole. */
  .cluster.area {
    border-radius: var(--radius-card);
    background: color-mix(in srgb, var(--color-fill) 60%, transparent);
  }

  /* A group that stands for a result leads with its title, which opens it. */
  .label.headed {
    gap: 8px;
    max-inline-size: calc(100% - var(--inset) * 2);
  }

  .title {
    min-inline-size: 0;
    padding: 0;
    overflow: hidden;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 600;
    text-overflow: ellipsis;
    pointer-events: auto;
    cursor: default;
  }

  .title:hover {
    color: var(--color-accent);
  }

  .count {
    flex: none;
  }

  .take {
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    line-height: 16px;
    pointer-events: auto;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .take:hover {
    background: var(--color-control-hover);
    color: var(--color-text);
  }

  .title:focus-visible,
  .take:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
    border-radius: var(--radius-inset);
  }

  .layer {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    max-inline-size: 200px;
    overflow: hidden;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 13px;
    text-overflow: ellipsis;
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

<script lang="ts">
  import { getContext } from "svelte";
  import Icon from "$shared/ui/Icon";
  import CardFrame from "./CardFrame.svelte";
  import {
    AirplaneTakeOff01Icon,
    Calendar03Icon,
    CheckmarkCircle02Icon,
    Doc01Icon,
    Home01Icon,
    Money03Icon,
    PassportIcon,
    Tick02Icon,
  } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { workTasksKey, type WorkTasks } from "../../lib/work-tasks";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const GLYPHS = {
    dates: Calendar03Icon,
    flight: AirplaneTakeOff01Icon,
    stay: Home01Icon,
    entry: PassportIcon,
    money: Money03Icon,
    document: Doc01Icon,
    check: CheckmarkCircle02Icon,
  } as const;
  const glyph = $derived(GLYPHS[item.step?.icon ?? "check"]);
  const tasks = getContext<WorkTasks | undefined>(workTasksKey);
  /** Read through the task session, so a step done in Tasks reads done here. */
  const task = $derived(tasks?.task(item.id));
  const done = $derived(task?.status === "done");
</script>

<!-- One thing to do, whole: what it is at a glance, then its words. -->
<CardFrame id={item.id} {selected} plain>
  <div class="step" class:done data-icon={item.step?.icon ?? "check"}>
    <header>
      <span class="tile" aria-hidden="true"><Icon icon={glyph} size={15} /></span>
      <span class="end">
        {#if item.step}<span class="index">{item.step.index}</span>{/if}
        {#if task}<button
            type="button"
            class="task nodrag nopan"
            data-status={task.status}
            aria-label={done ? m.work_step_task_done() : m.work_step_task_open()}
            title={done ? m.work_step_task_done() : m.work_step_task_open()}
            onclick={(event) => {
              event.stopPropagation();
              tasks?.open(task.id);
            }}
            ><span class="check" aria-hidden="true"
              >{#if done}<Icon icon={Tick02Icon} size={10} strokeWidth={2.6} />{/if}</span
            ></button
          >{/if}
      </span>
    </header>
    <p class="text">{item.step?.text ?? item.title}</p>
    {#if item.detail}<p class="detail">{item.detail}</p>{/if}
  </div>
</CardFrame>

<style>
  .step {
    display: flex;
    flex-direction: column;
    gap: 8px;
    box-sizing: border-box;
    block-size: 100%;
    min-block-size: 0;
    padding: 12px 12px 16px;
  }

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    flex: none;
  }

  .tile {
    display: grid;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-label-secondary);
  }

  .end {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .index {
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    line-height: 13px;
  }

  .text,
  .detail {
    margin: 0;
    overflow-wrap: anywhere;
    text-wrap: pretty;
  }

  .text {
    font-size: var(--text-label);
    font-weight: 500;
    line-height: 16px;
  }

  /* The tasks kit's check, read-only: it opens the task rather than ticking it. */
  .task {
    display: grid;
    place-items: center;
    inline-size: 24px;
    block-size: 24px;
    margin: -4px -4px -4px 0;
    padding: 0;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    cursor: default;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  .task:hover {
    background: var(--color-control-hover);
  }

  .task:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .check {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    inline-size: 15px;
    block-size: 15px;
    border: 1.25px solid var(--color-faint);
    border-radius: var(--radius-capsule);
    color: var(--color-on-lit);
  }

  .task[data-status="done"] .check {
    border-color: var(--color-lit);
    background: var(--color-lit);
  }

  .task[data-status="blocked"] .check {
    border-color: var(--color-warning);
  }

  /* As a finished task row reads: struck, on the faint rung. */
  .done .text {
    color: var(--color-faint);
    text-decoration: line-through;
    text-decoration-color: var(--color-faint);
  }

  .detail {
    margin-block-start: -4px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 15px;
  }
</style>

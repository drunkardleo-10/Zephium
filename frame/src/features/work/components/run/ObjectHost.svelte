<script lang="ts">
  import { getContext, untrack } from "svelte";
  import ObjectView from "../objects/ObjectView.svelte";
  import { canvasBoard, canvasOpen, type BoardActions } from "../../lib/canvas-context";
  import type { CanvasItem } from "../../lib/canvas-model";
  import type { Detail, ObjectActions, ObjectView as View } from "../../lib/board/types";
  import { workTasksKey, type WorkTasks } from "../../lib/work-tasks";

  let {
    id,
    item,
    selected = false,
    detail = "full",
  }: { id: string; item: CanvasItem; selected?: boolean; detail?: Detail } = $props();
  const board = getContext<BoardActions | undefined>(canvasBoard);
  const open = getContext<((id: string) => void) | undefined>(canvasOpen);
  const tasks = getContext<WorkTasks | undefined>(workTasksKey);
  /** A checkable plan's or list's steps, ticked where their task is done in Tasks. */
  const made = $derived(tasks ? tasks.tasks(id) : []);
  const view = $derived.by((): View => {
    const base = item.object!.view;
    if (!made.some(Boolean)) return base;
    const done = (index: number) => !!made[index]?.completedAt;
    if (base.kind === "plan")
      return {
        ...base,
        steps: base.steps.map((step, index) => (done(index) ? { ...step, done: true } : step)),
      };
    if (base.kind === "list")
      return {
        ...base,
        items: base.items.map((entry, index) => (done(index) ? { ...entry, done: true } : entry)),
      };
    return base;
  });
  const width = $derived(item.size?.width ?? 0);
  const actions: ObjectActions = {
    open: (target) => open?.(target),
    choose: (element, chosen) => board?.choose(element, chosen),
    ask: (subject) => board?.ask(subject),
    evidence: (reference) => board?.evidence(reference),
    link: (url) => board?.page(url),
    compare: (target) => board?.compare?.(target),
    send: (target) => board?.send?.(target),
    write: (target, markdown) => board?.write?.(target, markdown),
    // A step made into a task opens it in Tasks; its tick lives there.
    check: (_object, index) => {
      const task = made[index];
      if (task) tasks?.open(task.id);
    },
  };

  let body = $state<HTMLElement>();
  // The object's height at its width and detail: surveyed from afar it may
  // take more room, and the run makes it when the detail changes.
  let reported = "";
  $effect(() => {
    const element = body;
    const across = width;
    const level = detail;
    if (!element || view.state === "pending") return;
    const report = () => {
      const height = Math.ceil(element.offsetHeight);
      const key = `${across}|${height}|${level}`;
      if (!height || key === reported) return;
      reported = key;
      untrack(() => board?.measure(id, across, false, height, level));
    };
    report();
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(report);
    });
    observer.observe(element);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  });
</script>

<div class="object work-drag-handle {view.kind}" class:selected data-card-id={id}>
  <div bind:this={body}><ObjectView object={view} {detail} {actions} /></div>
</div>

<style>
  .object {
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    border-radius: var(--radius-card);
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .object.selected {
    box-shadow: 0 0 0 1.5px var(--color-ring);
  }
</style>

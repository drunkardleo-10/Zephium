<script lang="ts">
  import Menu from "$shared/ui/Menu";
  import Icon from "$shared/ui/Icon";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import { SvelteSet } from "svelte/reactivity";
  import {
    Alert02Icon,
    ArrowDown01Icon,
    ArrowUp01Icon,
    Calendar03Icon,
    CheckListIcon,
    CheckmarkCircle02Icon,
    CircleIcon,
    Clock01Icon,
    Flag02Icon,
    MoreHorizontalIcon,
    PinIcon,
    PlayCircleIcon,
    SparklesIcon,
  } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import type { TaskRow, TaskStatus } from "$domain/resources";
  import { createPointerDrag } from "$shared/lib/pointer-drag.svelte";
  import { compareRows, dueLabel, dueTone, durationLabel, hostOf } from "../lib/task-sections";
  import { today as currentDay, watchToday } from "../lib/today.svelte";
  import { orderBetween, placement } from "../lib/task-order";
  import { PRIORITY_ICON } from "../lib/priority";
  import { DUE_LABELS, DURATION_LABELS } from "../lib/labels";
  import TaskCheck from "./TaskCheck.svelte";
  import SiteMark from "./SiteMark.svelte";

  let {
    rows,
    selectedId = null,
    announce,
    onmove,
    onselect,
    onopenpage,
  }: {
    rows: readonly TaskRow[];
    selectedId?: string | null;
    announce?: (message: string) => void;
    /** State and position settle together; a column change is one write. */
    onmove: (id: string, status: TaskStatus, sortKey: string | null) => Promise<boolean>;
    onselect: (id: string) => void;
    onopenpage: (url: string) => void;
  } = $props();

  const COLUMNS: { id: TaskStatus; label: () => string; icon: IconSvgElement }[] = [
    { id: "open", label: m.task_column_open, icon: CircleIcon },
    { id: "active", label: m.task_column_active, icon: PlayCircleIcon },
    { id: "blocked", label: m.task_column_blocked, icon: Alert02Icon },
    { id: "done", label: m.task_column_done, icon: CheckmarkCircle02Icon },
  ];
  let today = $derived(currentDay());
  /** Cards whose menu has been reached; a board of many cards builds only those. */
  const armed = new SvelteSet<string>();
  $effect(() => watchToday());

  let columns = $derived(
    COLUMNS.map((column) => ({
      ...column,
      rows: rows.filter((row) => row.status === column.id).sort(compareRows),
    })),
  );

  /** Where the card under the pointer would land, drawn as a line. */
  let marker = $state<{ column: TaskStatus; before: string | null } | null>(null);

  const drag = createPointerDrag<TaskRow>({
    ondrop(task, target) {
      const landing = resolve(target);
      marker = null;
      if (!landing) return;
      const { status, over, after } = landing;
      const ids = columns.find((column) => column.id === status)!.rows.map((row) => row.id);
      const edges = placement(ids, task.id, over, after);
      const key = orderBetween(
        edges.before === null ? null : keyOf(edges.before),
        edges.after === null ? null : keyOf(edges.after),
      );
      if (key === null) {
        orderError = m.task_order_unavailable();
        return;
      }
      void commitMove(task, status, key);
    },
  });

  const keyOf = (id: string) => rows.find((row) => row.id === id)?.sortKey ?? null;
  let orderError = $state("");
  async function commitMove(task: TaskRow, status: TaskStatus, key: string | null) {
    orderError = "";
    if (await onmove(task.id, status, key))
      announce?.(
        m.task_announce_moved({
          title: task.title,
          column: COLUMNS.find((column) => column.id === status)!.label(),
        }),
      );
  }
  function moveBy(task: TaskRow, offset: number) {
    const column = columns.find((entry) => entry.id === task.status)!;
    const at = column.rows.findIndex((row) => row.id === task.id);
    const over = column.rows[at + offset];
    if (!over) return;
    const edges = placement(
      column.rows.map((row) => row.id),
      task.id,
      over.id,
      offset > 0,
    );
    const key = orderBetween(
      edges.before ? keyOf(edges.before) : null,
      edges.after ? keyOf(edges.after) : null,
    );
    if (key === null) {
      orderError = m.task_order_unavailable();
      return;
    }
    void commitMove(task, task.status, key);
  }

  /** Reads the element under the pointer as a column, and where within it. */
  function resolve(
    target: Element | null,
  ): { status: TaskStatus; over: string | null; after: boolean } | null {
    const column = target?.closest<HTMLElement>("[data-task-column]");
    const status = column?.dataset.taskColumn as TaskStatus | undefined;
    if (!status) return null;
    const card = target?.closest<HTMLElement>("[data-task-card]");
    if (!card) return { status, over: null, after: false };
    const box = card.getBoundingClientRect();
    return {
      status,
      over: card.dataset.taskCard ?? null,
      after: drag.at.y > box.top + box.height / 2,
    };
  }

  function hover(event: PointerEvent) {
    drag.move(event);
    if (!drag.item) return;
    const landing = resolve(document.elementFromPoint(drag.at.x, drag.at.y));
    marker = landing
      ? {
          column: landing.status,
          before: landing.over === null ? null : landing.after ? nextOf(landing) : landing.over,
        }
      : null;
  }

  function nextOf(landing: { status: TaskStatus; over: string | null }): string | null {
    const ids = columns.find((column) => column.id === landing.status)!.rows.map((row) => row.id);
    const at = ids.indexOf(landing.over ?? "");
    return at < 0 ? null : (ids[at + 1] ?? null);
  }
</script>

{#if orderError}<p class="board-error" role="alert">{orderError}</p>{/if}
<div class="board" role="group" aria-label={m.task_board()} data-dragging={drag.item !== null}>
  {#each columns as column (column.id)}
    <section
      class="board-column"
      data-task-column={column.id}
      data-over={drag.item !== null && marker?.column === column.id}
    >
      <header class="board-heading" data-status={column.id}>
        <Icon icon={column.icon} size={14} /><span>{column.label()}</span><small
          >{column.rows.length}</small
        >
      </header>
      <div class="board-cards">
        {#each column.rows as task (task.id)}
          {#if marker?.column === column.id && marker.before === task.id}
            <span class="board-marker" aria-hidden="true"></span>
          {/if}
          <article
            class="board-card"
            data-task-card={task.id}
            data-carrying={drag.item?.id === task.id}
            data-selected={selectedId === task.id}
            data-status={task.status}
            onpointerenter={() => armed.add(task.id)}
            onfocusin={() => armed.add(task.id)}
            onpointerdown={(event) => drag.begin(event, task)}
            onpointermove={hover}
            onpointerup={drag.end}
            onpointercancel={drag.cancel}
          >
            <div class="board-card-line">
              <TaskCheck
                status={task.status}
                label={task.title}
                ontoggle={(next) => void commitMove(task, next, task.sortKey)}
              />
              <button
                type="button"
                class="board-card-title"
                onclick={() => {
                  if (!drag.absorbClick()) onselect(task.id);
                }}
              >
                {#if task.origin === "agent"}<Icon
                    icon={SparklesIcon}
                    size={12}
                    label={m.task_from_agent()}
                  />{/if}<span>{task.title}</span>
              </button>
              {#if !armed.has(task.id)}<button
                  type="button"
                  class="board-card-menu"
                  aria-label={m.task_more()}><Icon icon={MoreHorizontalIcon} size={14} /></button
                >{:else}<Menu
                  label={m.task_more()}
                  entries={[
                    ...COLUMNS.map((entry) => ({
                      kind: "item" as const,
                      id: entry.id,
                      label: entry.label(),
                      icon: entry.icon,
                      checked: entry.id === task.status,
                    })),
                    { kind: "separator" },
                    {
                      kind: "item",
                      id: "up",
                      label: m.task_move_up(),
                      icon: ArrowUp01Icon,
                      disabled: column.rows[0]?.id === task.id,
                    },
                    {
                      kind: "item",
                      id: "down",
                      label: m.task_move_down(),
                      icon: ArrowDown01Icon,
                      disabled: column.rows.at(-1)?.id === task.id,
                    },
                  ]}
                  triggerClass="board-card-menu"
                  onselect={(id) => {
                    if (id === "up" || id === "down") moveBy(task, id === "up" ? -1 : 1);
                    else if (id !== task.status) void commitMove(task, id as TaskStatus, null);
                  }}
                >
                  {#snippet trigger()}<Icon icon={MoreHorizontalIcon} size={14} />{/snippet}
                </Menu>{/if}
            </div>
            {#if task.priority !== "none" || task.pinned || task.dueDate || task.deadline || task.duration || task.stepCount || task.context}
              <div class="board-card-meta">
                {#if task.priority !== "none"}<span data-priority={task.priority}
                    ><Icon
                      icon={PRIORITY_ICON[task.priority]}
                      size={12}
                      label={m.task_priority()}
                    /></span
                  >{/if}
                {#if task.pinned}<span
                    ><Icon icon={PinIcon} size={11} label={m.resource_pinned()} /></span
                  >{/if}
                {#if task.dueDate}<span
                    data-tone={task.status === "done" ? null : dueTone(task.dueDate, today)}
                    ><Icon icon={Calendar03Icon} size={11} />{dueLabel(
                      task.dueDate,
                      today,
                      DUE_LABELS,
                      task.dueTime,
                    )}</span
                  >{/if}
                {#if task.deadline}<span
                    class="board-deadline"
                    data-tone={task.status === "done" ? null : dueTone(task.deadline, today)}
                    aria-label={m.task_deadline_label({
                      date: dueLabel(task.deadline, today, DUE_LABELS),
                    })}
                    ><Icon icon={Flag02Icon} size={11} />{dueLabel(
                      task.deadline,
                      today,
                      DUE_LABELS,
                    )}</span
                  >{/if}
                {#if task.duration}<span
                    ><Icon icon={Clock01Icon} size={11} />{durationLabel(
                      task.duration,
                      DURATION_LABELS,
                    )}</span
                  >{/if}
                {#if task.stepCount}<span
                    aria-label={m.task_subtask_progress({
                      done: task.stepDone,
                      total: task.stepCount,
                    })}
                    ><Icon icon={CheckListIcon} size={11} />{task.stepDone}/{task.stepCount}</span
                  >{/if}
                {#if task.context}<button
                    type="button"
                    class="board-site"
                    title={task.context.url}
                    onclick={() => {
                      if (!drag.absorbClick()) onopenpage(task.context!.url);
                    }}
                    ><SiteMark url={task.context.url} size={11} />{hostOf(task.context.url)}</button
                  >{/if}
              </div>
            {/if}
          </article>
        {/each}
        {#if marker?.column === column.id && marker.before === null}
          <span class="board-marker" aria-hidden="true"></span>
        {/if}
        {#if !column.rows.length}
          <div class="board-empty" aria-hidden="true"></div>
        {/if}
      </div>
    </section>
  {/each}
</div>

{#if drag.item}
  <div class="board-ghost" style:translate={`${drag.at.x + 12}px ${drag.at.y + 8}px`}>
    {drag.item.title}
  </div>
{/if}

<style>
  .board-error {
    color: var(--color-danger);
    font-size: var(--text-label);
  }

  .board {
    display: grid;
    grid-auto-columns: minmax(208px, 1fr);
    grid-auto-flow: column;
    gap: 16px;
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow-x: auto;
    padding: 0 4px 16px;
  }

  .board-column {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    border-radius: var(--radius-card);
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  /* The column a card would land in lights, so the drop is never a guess. */
  .board-column[data-over="true"] {
    background: var(--row-hover);
  }

  .board-heading {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
    padding: 4px 6px 10px;
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .board-heading :global(svg) {
    color: var(--color-faint);
  }

  .board-heading[data-status="blocked"] :global(svg) {
    color: var(--color-warning);
  }

  .board-heading span {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .board-heading small {
    color: var(--color-faint);
    font-size: var(--text-label);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .board-cards {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: 1;
    min-height: 0;
    padding: 2px 2px 8px;
    overflow-y: auto;
  }

  .board-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: none;
    padding: 10px 8px 10px 10px;
    border-radius: var(--radius-row);
    background: var(--color-card);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    touch-action: none;
    transition:
      opacity var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out);
  }

  .board-card[data-selected="true"] {
    box-shadow:
      var(--shadow-raised),
      inset 0 0 0 1.5px var(--color-ring);
  }

  .board-card[data-carrying="true"] {
    opacity: 0.4;
  }

  .board-card-line {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    min-width: 0;
  }

  .board-card-line > :global(.task-check) {
    margin-block-start: 1px;
  }

  .board-card-title {
    display: flex;
    align-items: baseline;
    gap: 6px;
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    border-radius: 2px;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--text-body);
    line-height: 20px;
    text-align: start;
    cursor: default;
    outline: none;
  }

  .board-card-meta > span {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }

  .board-card-title > span {
    display: -webkit-box;
    min-width: 0;
    overflow: hidden;
    overflow-wrap: anywhere;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }

  .board-card[data-status="done"] .board-card-title > span {
    color: var(--color-faint);
    text-decoration: line-through;
  }

  .board-card-title:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .board-card :global(.board-card-menu) {
    display: grid;
    place-items: center;
    flex: none;
    width: 22px;
    height: 22px;
    margin-block-start: -1px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: var(--color-faint);
    opacity: 0;
    cursor: default;
    outline: none;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .board-card:hover :global(.board-card-menu),
  .board-card:focus-within :global(.board-card-menu),
  .board-card :global(.board-card-menu[data-state="open"]) {
    opacity: 1;
  }

  .board-card :global(.board-card-menu:hover) {
    background: var(--row-pressed);
    color: var(--color-text);
  }

  .board-card :global(.board-card-menu:focus-visible) {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .board-card-meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px 10px;
    padding-inline-start: 28px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .board-card-meta [data-priority="high"] {
    color: var(--color-text);
  }

  .board-card-meta [data-tone="overdue"] {
    color: var(--color-danger);
  }

  .board-card-meta .board-deadline[data-tone="today"] {
    color: var(--color-warning);
  }

  .board-site {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    max-width: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--text-caption);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: default;
    outline: none;
  }

  .board-site:hover {
    color: var(--color-text);
  }

  .board-site:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .board-marker {
    flex: none;
    height: 2px;
    margin-block: -4px;
    border-radius: var(--radius-capsule);
    background: var(--color-accent);
  }

  /* An empty column is quiet until something could land in it. */
  .board-empty {
    height: 56px;
    margin: 0;
    border-radius: var(--radius-row);
  }

  .board[data-dragging="true"] .board-empty {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  /* Follows the pointer and is never hit-tested, so the element under the
     pointer is always the drop target rather than the thing being dragged. */
  .board-ghost {
    position: fixed;
    top: 0;
    left: 0;
    z-index: 60;
    max-width: 240px;
    padding: 6px 10px;
    border-radius: var(--radius-row);
    background: var(--color-raised);
    box-shadow: var(--shadow-float);
    color: var(--color-text);
    font-size: var(--text-body);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
  }
</style>

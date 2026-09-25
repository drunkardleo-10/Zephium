<script lang="ts">
  import { Popover } from "bits-ui";
  import { untrack, type Snippet } from "svelte";
  import Icon from "$shared/ui/Icon";
  import {
    ArrowRight01Icon,
    ArrowLeft02Icon,
    Cancel01Icon,
    Moon02Icon,
    Sun03Icon,
    Calendar03Icon,
    Clock01Icon,
  } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import "$shared/ui/Menu/popover.css";
  import {
    addDays,
    dayName,
    dayNumber,
    fullDayName,
    inMonth,
    monthGrid,
    monthLabel,
    monthOf,
    nextWeek,
    nextWeekday,
    shiftMonth,
    weekStart,
    weekdayNames,
  } from "../lib/task-calendar";
  import { timeLabel } from "../lib/task-sections";
  import { parseTime } from "../lib/task-language";

  let {
    due,
    time = null,
    today,
    label,
    timeless = false,
    clearLabel = m.task_due_clear(),
    trigger,
    onselect,
  }: {
    due: string | null;
    time?: string | null;
    today: string;
    label: string;
    /** A day with no hour, as a deadline is. */
    timeless?: boolean;
    clearLabel?: string;
    trigger: Snippet<[{ props: Record<string, unknown> }]>;
    onselect: (day: string | null, time: string | null) => void;
  } = $props();

  let open = $state(false);
  const starts = weekStart();
  // The month the panel is looking at, which the reader can move away from the
  // one holding the current date. Seeded once here and reset on each open.
  // svelte-ignore state_referenced_locally
  let viewing = $state(monthOf(due ?? today));
  // svelte-ignore state_referenced_locally
  let focused = $state(due ?? today);
  let grid = $derived(monthGrid(viewing.year, viewing.month, starts));
  let headings = $derived(weekdayNames(starts));
  let cells: HTMLButtonElement[] = $state([]);
  // svelte-ignore state_referenced_locally
  let clock = $state(time === null ? "" : timeLabel(time));

  // Opening resets the view to the date being changed; nothing else does.
  $effect(() => {
    if (!open) return;
    untrack(() => {
      viewing = monthOf(due ?? today);
      focused = due ?? today;
      clock = time === null ? "" : timeLabel(time);
    });
  });

  const quick = $derived([
    { id: addDays(today, 0), icon: Sun03Icon, label: m.task_due_today(), hint: dayName(today) },
    {
      id: addDays(today, 1),
      icon: ArrowRight01Icon,
      label: m.task_due_tomorrow(),
      hint: dayName(addDays(today, 1)),
    },
    {
      id: nextWeekday(today, 6),
      icon: Moon02Icon,
      label: m.task_due_weekend(),
      hint: dayName(nextWeekday(today, 6)),
    },
    {
      id: nextWeek(today, starts),
      icon: Calendar03Icon,
      label: m.task_due_next_week(),
      hint: dayName(nextWeek(today, starts)),
    },
  ]);

  // Picking a day keeps the hour that was already on it; clearing the day takes
  // the hour with it, because a time with no date is not a moment.
  function choose(day: string | null) {
    open = false;
    onselect(day, day === null ? null : time);
  }

  // What the typed words will become, shown while typing so the field answers.
  let reading = $derived.by(() => {
    const parsed = parseTime(clock);
    return parsed !== null && clock.trim() !== timeLabel(parsed) ? timeLabel(parsed) : null;
  });

  // A typed time is kept until Enter or until the panel closes: committing on
  // blur can redraw the row that owns this panel mid-interaction.
  function setTime(input: string) {
    const parsed = parseTime(input);
    if (parsed === null && input.trim()) {
      clock = time === null ? "" : timeLabel(time);
      return;
    }
    clock = parsed === null ? "" : timeLabel(parsed);
    if (parsed === time) return;
    onselect(due ?? today, parsed);
  }

  function openChange(next: boolean) {
    if (next) return;
    const shown = time === null ? "" : timeLabel(time);
    if (clock.trim() !== shown) setTime(clock);
  }

  function move(days: number) {
    const next = addDays(focused, days);
    focused = next;
    if (!inMonth(next, viewing.year, viewing.month)) viewing = monthOf(next);
    // The cell exists only after the month it belongs to has drawn.
    queueMicrotask(() => cells.find((cell) => cell?.dataset.day === next)?.focus());
  }

  function keydown(event: KeyboardEvent) {
    const steps: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -7,
      ArrowDown: 7,
      PageUp: -28,
      PageDown: 28,
    };
    const step = steps[event.key];
    if (step === undefined) return;
    event.preventDefault();
    move(step);
  }

  function step(delta: number) {
    viewing = shiftMonth(viewing.year, viewing.month, delta);
  }
</script>

<Popover.Root bind:open onOpenChange={openChange}>
  <Popover.Trigger>
    {#snippet child({ props })}{@render trigger({ props })}{/snippet}
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content
      class="ui-menu ui-popover due-menu"
      sideOffset={6}
      align="end"
      aria-label={label}
    >
      <div class="due-quick">
        {#each quick as option (option.label)}
          <button type="button" class="due-option" onclick={() => choose(option.id)}>
            <Icon icon={option.icon} size={15} /><span>{option.label}</span><small
              >{option.hint}</small
            >
          </button>
        {/each}
      </div>
      <div class="due-month">
        <button
          type="button"
          class="due-step"
          aria-label={m.task_due_previous_month()}
          onclick={() => step(-1)}
        >
          <Icon icon={ArrowLeft02Icon} size={14} />
        </button>
        <span aria-live="polite">{monthLabel(viewing.year, viewing.month)}</span>
        <button
          type="button"
          class="due-step"
          aria-label={m.task_due_next_month()}
          onclick={() => step(1)}
        >
          <Icon icon={ArrowRight01Icon} size={14} />
        </button>
      </div>
      <div
        class="due-grid"
        role="grid"
        tabindex={-1}
        aria-label={monthLabel(viewing.year, viewing.month)}
        onkeydown={keydown}
      >
        <div class="due-headings" role="row">
          {#each headings as heading, index (index)}<span role="columnheader" title={heading}
              >{heading.slice(0, 2)}</span
            >{/each}
        </div>
        {#each grid as week, row (week[0])}
          <div class="due-week" role="row">
            {#each week as day, column (day)}
              <span role="gridcell">
                <button
                  bind:this={cells[row * 7 + column]}
                  type="button"
                  class="due-day"
                  data-day={day}
                  data-outside={!inMonth(day, viewing.year, viewing.month)}
                  data-today={day === today}
                  aria-label={fullDayName(day)}
                  aria-pressed={day === due}
                  tabindex={day === focused ? 0 : -1}
                  onclick={() => choose(day)}>{dayNumber(day)}</button
                >
              </span>
            {/each}
          </div>
        {/each}
      </div>
      {#if !timeless}<div class="due-time">
          <Icon icon={Clock01Icon} size={15} /><input
            type="text"
            aria-label={m.task_due_time()}
            placeholder={m.task_due_time_hint()}
            maxlength="16"
            value={clock}
            oninput={(event) => (clock = event.currentTarget.value)}
            onkeydown={(event) => {
              if (event.key !== "Enter") return;
              event.preventDefault();
              open = false;
              setTime(clock);
            }}
          />{#if reading}<span class="due-time-reading" aria-live="polite">{reading}</span
            >{/if}{#if time !== null}<button
              type="button"
              class="due-step"
              aria-label={m.task_due_clear_time()}
              onclick={() => {
                clock = "";
                open = false;
                onselect(due ?? today, null);
              }}><Icon icon={Cancel01Icon} size={13} /></button
            >{/if}
        </div>{/if}
      {#if due}
        <button type="button" class="due-option due-clear" onclick={() => choose(null)}>
          <Icon icon={Cancel01Icon} size={15} /><span>{clearLabel}</span>
        </button>
      {/if}
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>

<style>
  :global(.due-menu) {
    width: 264px;
    display: grid;
    gap: 8px;
  }

  .due-quick {
    display: grid;
    gap: 1px;
  }

  .due-option {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 30px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  .due-option span {
    flex: 1;
    min-width: 0;
  }

  .due-option small {
    color: var(--color-faint);
    font-size: var(--text-label);
  }

  .due-option:hover {
    background: var(--color-control-hover);
  }

  .due-clear {
    color: var(--color-muted);
    border-top: 1px solid var(--color-border);
    border-radius: 0 0 var(--radius-row) var(--radius-row);
    padding-top: 6px;
    margin-top: 2px;
  }

  /* A time is an hour on a day, so it sits with the calendar rather than in a
     separate destination. It reads what was typed: "3pm" and "15:00" both land. */
  .due-time {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 8px;
    border-top: 1px solid var(--color-border);
    color: var(--color-faint);
  }

  .due-time input {
    flex: 1;
    min-width: 0;
    height: 28px;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    outline: none;
  }

  .due-time input::placeholder {
    color: var(--color-faint);
  }

  .due-time input:focus-visible {
    outline: none;
  }

  .due-time-reading {
    flex: none;
    padding: 1px 6px;
    border-radius: var(--radius-inset);
    background: var(--color-accent-soft);
    color: var(--color-text);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
  }

  .due-month {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
    padding-inline: 2px;
    font-size: var(--text-label);
    font-weight: 550;
    letter-spacing: -0.01em;
  }

  .due-step {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: var(--color-muted);
    cursor: default;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  .due-step:hover {
    background: var(--color-control-hover);
    color: var(--color-text);
  }

  .due-headings,
  .due-week {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
  }

  .due-headings span {
    padding-block: 2px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    text-align: center;
  }

  .due-day {
    width: 100%;
    height: 30px;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
    cursor: default;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      color var(--motion-instant) var(--ease-smooth);
  }

  .due-day[data-outside="true"] {
    color: var(--color-faint);
  }

  .due-day:hover {
    background: var(--color-control-hover);
  }

  /* Today is marked, the chosen day is filled. Both at once still reads. */
  .due-day[data-today="true"] {
    font-weight: 600;
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .due-day[aria-pressed="true"] {
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  .due-option:focus-visible,
  .due-step:focus-visible,
  .due-day:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -1px;
  }

  @media (forced-colors: active) {
    .due-day[aria-pressed="true"] {
      background: Highlight;
      color: HighlightText;
    }
  }
</style>

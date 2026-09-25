import * as m from "$shared/i18n/messages";
import type { TaskPriority, TaskStatus } from "$domain/resources";
import type { DueLabels, DurationLabels } from "./task-sections";

/** Words every task surface draws, built once rather than once per row. Getters
 *  and functions, so each read follows the active locale. */
export const DUE_LABELS: DueLabels = {
  get today() {
    return m.task_due_today();
  },
  get tomorrow() {
    return m.task_due_tomorrow();
  },
  get yesterday() {
    return m.task_due_yesterday();
  },
};

export const DURATION_LABELS: DurationLabels = {
  minutes: (count) => m.task_duration_minutes({ count }),
  hours: (count) => m.task_duration_hours({ count }),
  mixed: (hours, minutes) => m.task_duration_mixed({ hours, minutes }),
};

export const PRIORITY_LABEL: Record<TaskPriority, () => string> = {
  high: m.task_priority_high,
  medium: m.task_priority_medium,
  low: m.task_priority_low,
  none: m.task_priority_none,
};

export const STATE_LABEL: Record<TaskStatus, () => string> = {
  open: m.task_state_open,
  active: m.task_state_active,
  blocked: m.task_state_blocked,
  done: m.task_state_done,
};

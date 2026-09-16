import type { ToolKind } from "$shared/ipc/bindings";
import * as m from "$shared/i18n/messages";
import {
  Note01Icon,
  Task01Icon,
  SparklesIcon,
  Clock01Icon,
  Download01Icon,
} from "@hugeicons/core-free-icons";
export const tools = {
  notes: {
    title: m.tool_notes,
    description: m.tool_notes_help,
    empty: m.tool_notes_empty,
    icon: Note01Icon,
    load: () => import("./previews/NotesView.svelte"),
  },
  tasks: {
    title: m.tool_tasks,
    description: m.tool_tasks_help,
    empty: m.tool_tasks_empty,
    icon: Task01Icon,
    load: () => import("./previews/TasksView.svelte"),
  },
  ai: {
    title: m.tool_ai,
    description: m.tool_ai_help,
    empty: m.tool_ai_empty,
    icon: SparklesIcon,
    load: () => import("./previews/ChatView.svelte"),
  },
  history: {
    title: m.browser_history_title,
    description: m.history_empty_help,
    empty: m.browser_history_empty,
    icon: Clock01Icon,
    load: () => import("./previews/HistoryView.svelte"),
  },
  downloads: {
    title: m.browser_downloads_title,
    description: m.browser_downloads_unavailable,
    empty: m.browser_downloads_empty,
    icon: Download01Icon,
    load: () => import("./previews/DownloadsView.svelte"),
  },
  time: {
    title: m.tool_time,
    description: m.tool_time_help,
    empty: m.tool_time_empty,
    icon: Clock01Icon,
    load: () => import("./previews/TimeView.svelte"),
  },
} satisfies Record<ToolKind, unknown>;
export const toolKinds = Object.keys(tools) as ToolKind[];

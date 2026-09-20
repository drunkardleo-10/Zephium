import * as m from "$shared/i18n/messages";
import {
  Activity03Icon,
  CheckListIcon,
  Download01Icon,
  HistoryIcon,
  Note01Icon,
  SparklesIcon,
} from "@hugeicons/core-free-icons";
import type { IconSvgElement } from "@hugeicons/svelte";
import type { ToolKind } from "$session/tools.svelte";

/**
 * How a tool is drawn. The art matches the tool panel's own header, so a
 * shelf glyph and the panel it opens can never disagree about what a tool is.
 */
const PRESENTATION: Record<ToolKind, { icon: IconSvgElement; label: () => string }> = {
  notes: { icon: Note01Icon, label: m.panel_notes },
  tasks: { icon: CheckListIcon, label: m.panel_tasks },
  time: { icon: Activity03Icon, label: m.panel_time },
  ai: { icon: SparklesIcon, label: m.panel_ai },
  history: { icon: HistoryIcon, label: m.menu_history },
  downloads: { icon: Download01Icon, label: m.menu_downloads },
};

export const toolPresentation = (kind: ToolKind) => PRESENTATION[kind];

/**
 * The mini-apps the shelf reveals on hover: the things you make or ask for,
 * rather than the things that merely happened to you. They follow the native
 * menu's own order, so the stack and the menu never disagree. History,
 * downloads and the rest stay in that menu, which is the whole list.
 */
export const SHELF_TOOLS = ["notes", "tasks", "time", "ai"] as const satisfies readonly ToolKind[];

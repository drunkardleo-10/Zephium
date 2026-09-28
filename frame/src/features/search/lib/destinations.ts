import { CheckListIcon, Download01Icon, HistoryIcon, Note01Icon } from "@hugeicons/core-free-icons";
import * as m from "$shared/i18n/messages";
import { IS_MAC } from "$shared/platform";
import type { Destination } from "./search-surface.svelte";

/** Places the launcher hands to the browser, which already has their views
 *  loaded. Only their names travel here: the launcher never pays for an
 *  editor or a task list a second time. Each answers to Command and its
 *  position, from anywhere in the launcher. */
export const launcherDestinations = (): Destination[] =>
  [
    { kind: "tasks" as const, label: m.tool_tasks(), icon: CheckListIcon },
    { kind: "notes" as const, label: m.tool_notes(), icon: Note01Icon },
    { kind: "history" as const, label: m.browser_history_title(), icon: HistoryIcon },
    { kind: "downloads" as const, label: m.browser_downloads_title(), icon: Download01Icon },
  ].map((destination, index) => ({
    ...destination,
    keys: [IS_MAC ? "⌘" : "Ctrl", String(index + 1)],
  }));

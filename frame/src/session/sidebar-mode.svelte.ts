import { transition } from "./motion.svelte";
import { commands } from "$shared/ipc/bindings";

export type SidebarMode = "default" | "compact";

/** The rail width. Native admits it as a real sidebar width, not a mode flag. */
export const COMPACT_WIDTH = 56;
export const MIN_EXPANDED_WIDTH = 180;
export const MAX_EXPANDED_WIDTH = 420;

/**
 * Dragging the handle below this snaps to the rail, and dragging back out of
 * the rail snaps to the minimum expanded width. Nothing settles in the gap,
 * so the sidebar is always in one of its two designed shapes.
 */
export const SNAP_THRESHOLD = 140;

const MODE_SETTING = "sidebar.mode";

let mode = $state.raw<SidebarMode>("default");
let desiredMode: SidebarMode = "default";
let expandedWidth = $state.raw(240);

export const sidebarMode = () => mode;
export const isCompact = () => mode === "compact";
export const expanded = () => expandedWidth;
/** The chrome gap between the rail and an open utility panel. */
const PANEL_GAP = 8;
let panelExtent = $state.raw(0);

export const effectiveWidth = () =>
  panelExtent > 0
    ? COMPACT_WIDTH + PANEL_GAP + panelExtent
    : mode === "compact"
      ? COMPACT_WIDTH
      : expandedWidth;

/** A utility panel borrows width beside the rail without changing the persisted mode. */
export function setPanelExtent(extent: number) {
  const next = Math.max(0, Math.min(MAX_EXPANDED_WIDTH - COMPACT_WIDTH - PANEL_GAP, extent));
  if (next === panelExtent) return;
  panelExtent = next;
  publish();
}

function clampExpanded(value: number) {
  return Math.max(MIN_EXPANDED_WIDTH, Math.min(MAX_EXPANDED_WIDTH, Math.round(value)));
}

/** Resolves a raw drag width to whichever designed shape it lands in. */
export function resolveDragWidth(value: number): { mode: SidebarMode; expanded: number } {
  if (!Number.isFinite(value) || value < SNAP_THRESHOLD) {
    return { mode: "compact", expanded: expandedWidth };
  }
  return { mode: "default", expanded: clampExpanded(value) };
}

function publish() {
  void commands.sidebarSetWidth(effectiveWidth());
}

export function applyDragWidth(value: number) {
  const next = resolveDragWidth(value);
  if (next.mode === mode && next.expanded === expandedWidth) return;

  const modeChanged = next.mode !== mode;
  mode = next.mode;
  expandedWidth = next.expanded;
  publish();
  if (modeChanged) void commands.settingSet(MODE_SETTING, mode);
}

export function adoptMode(next: SidebarMode) {
  if (next === mode) return;
  mode = next;
  desiredMode = next;
  publish();
}

export function setMode(next: SidebarMode) {
  if (next === mode) return;
  mode = next;
  desiredMode = next;
  publish();
  void commands.settingSet(MODE_SETTING, next);
}

export function toggleMode() {
  desiredMode = desiredMode === "compact" ? "default" : "compact";
  const next = desiredMode;
  transition(() => setMode(next));
}

/**
 * Adopts the persisted mode. The width is published afterwards so native
 * layout matches the restored shape on the first paint rather than after the
 * first interaction.
 */
export async function init(): Promise<void> {
  try {
    const stored = await commands.settingGet(MODE_SETTING);
    if (stored === "compact") {
      mode = "compact";
      desiredMode = "compact";
    }
  } catch {
    // A missing or unreadable preference is not a startup failure; the
    // default shape is always valid.
  }
  publish();
}

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

export const hasPanel = () => panelExtent > 0;

/** A utility panel borrows width beside the rail without changing the persisted mode. */
export function setPanelExtent(extent: number) {
  const next = Math.max(0, Math.min(MAX_EXPANDED_WIDTH - COMPACT_WIDTH - PANEL_GAP, extent));
  if (next === panelExtent) return;
  panelExtent = next;
  publish(true);
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

/**
 * Tells native the column's width. `travel` marks a deliberate change of
 * shape — a toggle, a snap, a tool opening — which the page slides with;
 * a drag in progress or a restored preference moves it without ceremony.
 */
function publish(travel = false) {
  void commands.sidebarSetWidth(effectiveWidth(), travel);
}

export function applyDragWidth(value: number) {
  const next = resolveDragWidth(value);
  if (next.mode === mode && next.expanded === expandedWidth) return;

  const modeChanged = next.mode !== mode;
  mode = next.mode;
  expandedWidth = next.expanded;
  // Crossing the snap point is a change of shape, not a drag step.
  publish(modeChanged);
  if (modeChanged) save(mode);
}

// The shape this column last saved, until the store reports it back. Values
// arriving meanwhile are the store catching up with saves made here, not a
// change from elsewhere: adopting them would snap the column back to a shape
// it has already left. A save whose report never comes stops counting.
let saving: { mode: SidebarMode; timer: ReturnType<typeof setTimeout> } | null = null;

function save(next: SidebarMode) {
  if (saving) clearTimeout(saving.timer);
  saving = { mode: next, timer: setTimeout(() => (saving = null), 2000) };
  void commands.settingSet(MODE_SETTING, next);
}

/** A stored preference reached this column: from here, or from elsewhere. */
export function adoptMode(next: SidebarMode) {
  if (saving) {
    if (next === saving.mode) {
      clearTimeout(saving.timer);
      saving = null;
    }
    return;
  }
  if (next === mode) return;
  mode = next;
  desiredMode = next;
  publish();
}

export function setMode(next: SidebarMode) {
  if (next === mode) return;
  mode = next;
  desiredMode = next;
  publish(true);
  save(next);
}

export function toggleMode() {
  desiredMode = desiredMode === "compact" ? "default" : "compact";
  setMode(desiredMode);
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

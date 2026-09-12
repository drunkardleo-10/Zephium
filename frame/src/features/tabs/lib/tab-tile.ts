import type { TabView } from "$shared/ipc/bindings";
/** A tab-list renderer slot. The host chooses its product presentation. */
export type TabTileProps = {
  tab: TabView;
  active: boolean;
  splitCandidate: boolean;
  onSelect: (id: string) => void;
  onContextMenu: (event: MouseEvent, tab: TabView) => void;
  onPointerDown: (event: PointerEvent, tab: TabView) => void;
  onPointerMove: (event: PointerEvent) => void;
  onPointerUp: (event: PointerEvent) => void;
  onPointerCancel: (event: PointerEvent) => void;
};

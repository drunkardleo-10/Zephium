import { createStore, reconcile } from "solid-js/store";
import type { ItemsState, TabView } from "../ipc/bindings";
import { commands } from "../ipc/bindings";
import { events } from "../ipc/native-events";

const [state, setState] = createStore<ItemsState>({
  projection_revision: "00000000000000000000000000000000",
  tabs: [],
  active: null,
});
let appliedGlobalRevision = state.projection_revision;
const appliedTabRevisions = new Map<string, string>();

function applyTabRevision(tab: TabView): boolean {
  const previous = appliedTabRevisions.get(tab.id) ?? "00000000000000000000000000000000";
  if (tab.projection_revision <= previous) return false;
  appliedTabRevisions.set(tab.id, tab.projection_revision);
  if (tab.projection_revision > appliedGlobalRevision) {
    appliedGlobalRevision = tab.projection_revision;
  }
  return true;
}

export const tabs = () => state.tabs;
export const activeId = () => state.active;
export const activeTab = (): TabView | undefined => state.tabs.find((t) => t.id === state.active);

let unlisten: (() => void) | null = null;

export async function init() {
  const unItems = await events.itemsChanged.listen((e) => {
    if (e.payload.projection_revision <= appliedGlobalRevision) return;
    appliedGlobalRevision = e.payload.projection_revision;
    for (const tab of e.payload.tabs) {
      const previous = appliedTabRevisions.get(tab.id);
      if (previous == null || tab.projection_revision > previous) {
        appliedTabRevisions.set(tab.id, tab.projection_revision);
      }
    }
    setState(reconcile(e.payload, { key: "id" }));
  });
  const unTab = await events.tabChanged.listen((e) => {
    const tab = e.payload;
    if (!applyTabRevision(tab)) return;
    setState("tabs", (t) => t.id === tab.id, reconcile(tab));
  });
  const unPresentationTab = await events.presentationTab.listen((e) => {
    const { tab, active } = e.payload;
    // Unrelated-tab deltas must not starve an exact first-paint barrier. Full
    // snapshots update every row's revision, so they still prevent an older
    // presentation from regressing tab or active state.
    if (!applyTabRevision(tab)) return;
    setState("tabs", (t) => t.id === tab.id, reconcile(tab));
    setState("active", active);
  });
  unlisten = () => {
    unItems();
    unTab();
    unPresentationTab();
  };
  // a cold first run can outrace tauri's setup, the command rejects until then
  for (let attempt = 0; attempt < 20; attempt++) {
    try {
      await commands.tabsBootstrap();
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 250));
    }
  }
}

export function dispose() {
  unlisten?.();
  unlisten = null;
}

export const open = () => void commands.tabsOpen();
export const activate = (id: string) => void commands.tabsActivate(id);
export const close = (id: string) => void commands.tabsClose(id);
export const navigate = (id: string, input: string) => void commands.tabsNavigate(id, input);

const onActive = (fn: (id: string) => void) => () => {
  const id = state.active;
  if (id != null) fn(id);
};
export const reloadActive = onActive((id) => void commands.tabsReload(id));
export const backActive = onActive((id) => void commands.tabsBack(id));
export const forwardActive = onActive((id) => void commands.tabsForward(id));
export const split = (other: string) => void commands.tabsSplit(other);
export const unsplit = () => void commands.tabsUnsplit();
export const setSidebarWidth = (width: number) => void commands.sidebarSetWidth(width);
export const dragOver = (x: number, y: number) => void commands.tabDragOver(x, y);
export const dropTab = (id: string, x: number, y: number) => void commands.tabDrop(id, x, y);

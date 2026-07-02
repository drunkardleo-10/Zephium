import { createStore, reconcile } from "solid-js/store";
import * as ipc from "../ipc/commands";
import type { TabView, TabsSnapshot } from "../ipc/types";

const [state, setState] = createStore<TabsSnapshot>({ tabs: [], active: null });

export const tabs = () => state.tabs;
export const activeId = () => state.active;
export const activeTab = (): TabView | undefined =>
  state.tabs.find((t) => t.id === state.active);

let unlisten: (() => void) | null = null;

export async function init() {
  unlisten = await ipc.onTabsState((s) => setState(reconcile(s, { key: "id" })));
  await ipc.bootstrap();
}

export function dispose() {
  unlisten?.();
  unlisten = null;
}

export const open = () => void ipc.openTab();
export const activate = (id: string) => void ipc.activateTab(id);
export const close = (id: string) => void ipc.closeTab(id);
export const navigate = (id: string, input: string) => void ipc.navigate(id, input);

const onActive = (fn: (id: string) => void) => () => {
  const id = state.active;
  if (id != null) fn(id);
};
export const reloadActive = onActive(ipc.reload);
export const backActive = onActive(ipc.back);
export const forwardActive = onActive(ipc.forward);
export const split = (other: string) => void ipc.split(other);
export const unsplit = () => void ipc.unsplit();
export const setSidebarWidth = (width: number) => void ipc.setSidebarWidth(width);
export const dragOver = (x: number, y: number) => void ipc.dragOver(x, y);
export const dropTab = (id: string, x: number, y: number) => void ipc.dropTab(id, x, y);

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
export const activate = (id: number) => void ipc.activateTab(id);
export const close = (id: number) => void ipc.closeTab(id);
export const navigate = (id: number, input: string) => void ipc.navigate(id, input);

const onActive = (fn: (id: number) => void) => () => {
  const id = state.active;
  if (id != null) fn(id);
};
export const reloadActive = onActive(ipc.reload);
export const backActive = onActive(ipc.back);
export const forwardActive = onActive(ipc.forward);
export const split = () => void ipc.split();
export const unsplit = () => void ipc.unsplit();

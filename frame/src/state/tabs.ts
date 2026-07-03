import { createStore, reconcile } from "solid-js/store";
import type { ItemsState, TabView } from "../ipc/bindings";
import { commands, events } from "../ipc/bindings";

const [state, setState] = createStore<ItemsState>({ tabs: [], active: null });

export const tabs = () => state.tabs;
export const activeId = () => state.active;
export const activeTab = (): TabView | undefined => state.tabs.find((t) => t.id === state.active);

let unlisten: (() => void) | null = null;

export async function init() {
  const unItems = await events.itemsChanged.listen((e) =>
    setState(reconcile(e.payload, { key: "id" })),
  );
  const unTab = await events.tabChanged.listen((e) => {
    const tab = e.payload;
    setState("tabs", (t) => t.id === tab.id, reconcile(tab));
  });
  unlisten = () => {
    unItems();
    unTab();
  };
  await commands.tabsBootstrap();
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

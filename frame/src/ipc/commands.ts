import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TabsSnapshot } from "./types";

export const bootstrap = () => invoke<void>("tabs_bootstrap");
export const openTab = () => invoke<void>("tabs_open");
export const activateTab = (id: string) => invoke<void>("tabs_activate", { id });
export const closeTab = (id: string) => invoke<void>("tabs_close", { id });
export const navigate = (id: string, input: string) => invoke<void>("tabs_navigate", { id, input });
export const reload = (id: string) => invoke<void>("tabs_reload", { id });
export const back = (id: string) => invoke<void>("tabs_back", { id });
export const forward = (id: string) => invoke<void>("tabs_forward", { id });
export const split = (other: string) => invoke<void>("tabs_split", { other });
export const unsplit = () => invoke<void>("tabs_unsplit");
export const setSidebarWidth = (width: number) => invoke<void>("sidebar_set_width", { width });
export const dragOver = (x: number, y: number) => invoke<void>("tab_drag_over", { x, y });
export const dropTab = (id: string, x: number, y: number) => invoke<void>("tab_drop", { id, x, y });

export const onTabsState = (cb: (s: TabsSnapshot) => void): Promise<UnlistenFn> =>
  listen<TabsSnapshot>("tabs:state", (e) => cb(e.payload));

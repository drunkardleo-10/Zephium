import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TabsSnapshot } from "./types";

export const bootstrap = () => invoke<void>("tabs_bootstrap");
export const openTab = () => invoke<void>("tabs_open");
export const activateTab = (id: number) => invoke<void>("tabs_activate", { id });
export const closeTab = (id: number) => invoke<void>("tabs_close", { id });
export const navigate = (id: number, input: string) => invoke<void>("tabs_navigate", { id, input });
export const reload = (id: number) => invoke<void>("tabs_reload", { id });
export const back = (id: number) => invoke<void>("tabs_back", { id });
export const forward = (id: number) => invoke<void>("tabs_forward", { id });
export const split = (other: number) => invoke<void>("tabs_split", { other });
export const unsplit = () => invoke<void>("tabs_unsplit");

export const onTabsState = (cb: (s: TabsSnapshot) => void): Promise<UnlistenFn> =>
  listen<TabsSnapshot>("tabs:state", (e) => cb(e.payload));

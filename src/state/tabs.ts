import { createStore } from "solid-js/store";
import * as wv from "../ipc/webview";

export const DEFAULT_URL = "https://duckduckgo.com";

export interface Tab {
  id: number;
  title: string;
  url: string;
  loading: boolean;
}

interface State {
  tabs: Tab[];
  activeId: number;
}

const [state, setState] = createStore<State>({ tabs: [], activeId: -1 });

let nextId = 1;

export const tabs = () => state.tabs;
export const activeId = () => state.activeId;
export const activeTab = (): Tab | undefined =>
  state.tabs.find((t) => t.id === state.activeId);

export function openTab(url = DEFAULT_URL): number {
  const id = nextId++;
  setState("tabs", (prev) => [...prev, { id, title: "New Tab", url, loading: true }]);
  void wv.createTab(id, url);
  activate(id);
  return id;
}

export function activate(id: number): void {
  if (state.activeId === id) return;
  setState("activeId", id);
  void wv.activateTab(id);
}

export function close(id: number): void {
  const idx = state.tabs.findIndex((t) => t.id === id);
  if (idx < 0) return;

  const wasActive = state.activeId === id;
  void wv.closeTab(id);
  setState("tabs", (prev) => prev.filter((t) => t.id !== id));

  if (state.tabs.length === 0) {
    openTab();
    return;
  }
  if (wasActive) {
    const next = state.tabs[Math.min(idx, state.tabs.length - 1)];
    activate(next.id);
  }
}

export function navigate(id: number, url: string): void {
  setState("tabs", (t) => t.id === id, { url, loading: true });
  void wv.navigate(id, url);
}

export const reloadActive = () => state.activeId >= 0 && wv.reload(state.activeId);
export const backActive = () => state.activeId >= 0 && wv.back(state.activeId);
export const forwardActive = () => state.activeId >= 0 && wv.forward(state.activeId);

export function applyUrl(id: number, url: string): void {
  setState("tabs", (t) => t.id === id, "url", url);
}

export function applyTitle(id: number, title: string): void {
  setState("tabs", (t) => t.id === id, "title", title || "Untitled");
}

export function applyLoading(id: number, loading: boolean): void {
  setState("tabs", (t) => t.id === id, "loading", loading);
}

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const createTab = (id: number, url: string): Promise<void> =>
  invoke("webview_create", { id, url });

export const activateTab = (id: number): Promise<void> =>
  invoke("webview_activate", { id });

export const closeTab = (id: number): Promise<void> =>
  invoke("webview_close", { id });

export const navigate = (id: number, url: string): Promise<void> =>
  invoke("webview_navigate", { id, url });

export const reload = (id: number): Promise<void> =>
  invoke("webview_reload", { id });

export const back = (id: number): Promise<void> =>
  invoke("webview_back", { id });

export const forward = (id: number): Promise<void> =>
  invoke("webview_forward", { id });

export const setContentBounds = (rect: Rect): Promise<void> =>
  invoke("webview_set_content_bounds", { ...rect });

interface UrlPayload {
  id: number;
  url: string;
}

interface TitlePayload {
  id: number;
  title: string;
}

interface LoadingPayload {
  id: number;
  loading: boolean;
}

export const onUrl = (cb: (p: UrlPayload) => void): Promise<UnlistenFn> =>
  listen<UrlPayload>("tab:url", (e) => cb(e.payload));

export const onTitle = (cb: (p: TitlePayload) => void): Promise<UnlistenFn> =>
  listen<TitlePayload>("tab:title", (e) => cb(e.payload));

export const onLoading = (cb: (p: LoadingPayload) => void): Promise<UnlistenFn> =>
  listen<LoadingPayload>("tab:loading", (e) => cb(e.payload));

const SCHEME = /^[a-z][a-z0-9+.-]*:\/\//i;
const LOOKS_LIKE_HOST = /^[^\s/]+\.[^\s/]{2,}/;

export function normalizeUrl(input: string): string {
  const s = input.trim();
  if (SCHEME.test(s)) return s;
  if (LOOKS_LIKE_HOST.test(s) || s.startsWith("localhost")) return `https://${s}`;
  return `https://duckduckgo.com/?q=${encodeURIComponent(s)}`;
}

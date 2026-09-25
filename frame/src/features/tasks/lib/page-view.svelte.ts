import { SvelteMap } from "svelte/reactivity";
import type { TaskScope } from "./task-sections";

/** Profile-local presentation state, shared by task navigation and the page.
 * Durable task/list membership remains in Rust. */
export type PageView = { scope: TaskScope; trashed: boolean; board: boolean; list: string | null };
const initial = (): PageView => ({ scope: "today", trashed: false, board: false, list: null });
const views = new SvelteMap<string, PageView>();
let owner = "unbound";
let view = $state<PageView>(initial());
export const pageView = () => view;
export function setPageProfile(profile: string) {
  if (owner === profile) return;
  views.set(owner, view);
  owner = profile;
  view = views.get(profile) ?? initial();
}
export function setPageView(patch: Partial<PageView>, profile?: string) {
  if (profile) setPageProfile(profile);
  view = { ...view, ...patch };
}

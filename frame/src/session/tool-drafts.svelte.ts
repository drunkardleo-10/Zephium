import { SvelteMap } from "svelte/reactivity";
import type { ToolKind } from "$shared/ipc/bindings";
export type ToolViewState = {
  query: string;
  filter: string;
  title: string;
  draft: string;
  composing: boolean;
  submitted: boolean;
  scrollTop: number;
};
export type ToolHostProps = {
  profile: string;
  tool: ToolKind;
  state: Readonly<ToolViewState>;
  edit: (patch: Partial<ToolViewState>) => void;
  profileName?: string;
  onclose: () => void;
};
const LIMIT = 24;
const sessions = new SvelteMap<string, ToolViewState>();
export function toolSession(profile: string, tool: ToolKind): ToolViewState {
  const key = `${profile}:${tool}`;
  const existing = sessions.get(key);
  if (existing) {
    // Reinserted on every read, so the bound evicts the least recently used
    // draft rather than the oldest one, which may belong to the open view.
    sessions.delete(key);
    sessions.set(key, existing);
    return existing;
  }
  const state = $state<ToolViewState>({
    query: "",
    // Tasks opens on what is due now; a list that opens on everything is a
    // list you have to narrow before you can read it.
    filter: tool === "time" || tool === "tasks" ? "today" : "all",
    title: "",
    draft: "",
    composing: false,
    submitted: false,
    scrollTop: 0,
  });
  if (sessions.size >= LIMIT) {
    const oldest = sessions.keys().next().value;
    if (oldest) sessions.delete(oldest);
  }
  sessions.set(key, state);
  return state;
}
export function editTool(state: ToolViewState, patch: Partial<ToolViewState>) {
  if (patch.query !== undefined) state.query = patch.query.slice(0, 1024);
  if (patch.title !== undefined) state.title = patch.title.slice(0, 256);
  if (patch.draft !== undefined) state.draft = patch.draft.slice(0, 16384);
  if (patch.filter !== undefined) state.filter = patch.filter.slice(0, 32);
  if (patch.composing !== undefined) state.composing = patch.composing;
  if (patch.submitted !== undefined) state.submitted = patch.submitted;
  if (patch.scrollTop !== undefined)
    state.scrollTop = Number.isFinite(patch.scrollTop) ? Math.max(0, patch.scrollTop) : 0;
}

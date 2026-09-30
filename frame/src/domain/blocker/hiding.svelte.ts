import type { BlockerSiteContext } from "$shared/ipc/bindings";
import { status } from "./blocker.svelte";
import { changeSite, picker } from "./site-actions";

// The page reports a pick only when asked. Asking this often, and only while
// hiding is on, keeps a click feeling instant without an idle cost.
const READ_MS = 200;

let active = $state(false);
let saving = $state(false);
let owner: BlockerSiteContext | null = null;
let session: string | null = null;
let baseline: string[] = [];
let timer: ReturnType<typeof setTimeout> | undefined;
let lifetime = 0;
let misses = 0;
let ended: (() => void) | null = null;

export const isActive = () => active;
export const isSaving = () => saving;

/** Hides saved since hiding started, oldest first. */
export const added = () =>
  active ? (status().site?.hides.filter((hide) => !baseline.includes(hide.id)) ?? []) : [];

// Picks, saves and undos must target the document hiding started on; the
// context's revision moves with every saved change, so always take it fresh.
function context(owner: BlockerSiteContext): BlockerSiteContext | null {
  const current = status().site?.context;
  return current &&
    current.profile === owner.profile &&
    current.tab === owner.tab &&
    current.site === owner.site
    ? current
    : null;
}

/** `onEnd` runs once when this hiding session ends, however it ends. */
export async function start(site: BlockerSiteContext, onEnd?: () => void): Promise<boolean> {
  finish();
  const generation = ++lifetime;
  const view = await picker(site, { kind: "start" });
  if (generation !== lifetime || !view?.active) return false;
  ended = onEnd ?? null;
  owner = site;
  session = view.session;
  baseline = status().site?.hides.map((hide) => hide.id) ?? [];
  misses = 0;
  active = true;
  schedule(generation);
  return true;
}

function schedule(generation: number) {
  clearTimeout(timer);
  timer = setTimeout(() => void read(generation), READ_MS);
}

async function read(generation: number) {
  const site = owner && context(owner);
  if (generation !== lifetime || !site || session === null) return finish();
  const view = await picker(site, { kind: "read", session });
  if (generation !== lifetime) return;
  if (!view) {
    // One slow answer is not the end of hiding; two in a row is.
    if (++misses > 1) return finish();
    return schedule(generation);
  }
  misses = 0;
  if (!view.active) return finish();
  if (view.selection) await save(generation, site, view.session, view.selection.identity);
  if (generation === lifetime) schedule(generation);
}

async function save(generation: number, site: BlockerSiteContext, pick: string, selection: string) {
  saving = true;
  await changeSite(site, { kind: "save_selection", session: pick, selection });
  saving = false;
  if (generation !== lifetime || !owner) return;
  // A fresh session clears the pick so the next click can be taken. The
  // element stays hidden in the page until the saved rule replaces it.
  const next = context(owner);
  const view = next ? await picker(next, { kind: "start" }) : null;
  if (generation !== lifetime) return;
  if (!view?.active) return finish();
  session = view.session;
}

export async function undo(): Promise<void> {
  const last = added().at(-1);
  const site = owner && context(owner);
  if (!last || !site || saving) return;
  saving = true;
  await changeSite(site, { kind: "remove_hide", id: last.id });
  saving = false;
}

export function finish(): void {
  lifetime += 1;
  clearTimeout(timer);
  const site = owner && context(owner);
  if (site && session !== null) void picker(site, { kind: "stop", session });
  owner = null;
  session = null;
  active = false;
  saving = false;
  const end = ended;
  ended = null;
  end?.();
}

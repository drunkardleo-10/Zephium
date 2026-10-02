import { observe, pause } from "$shared/lib/observe";
import { commands } from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import { settle } from "$domain/operations";

const defaults = {
  appearance: "system",
  "search.engine": "duckduckgo",
  "search.custom-url": "",
  "search.suggestions": "true",
  "sidebar.mode": "default",
  "ui.accent": "graphite",
  "ui.reduce-motion": "false",
  "ui.newtab-greeting": "false",
  "ui.newtab-name": "false",
  "ui.newtab-clock": "true",
  "ui.newtab-clock-format": "system",
  "ui.newtab-tasks": "true",
  "ui.tab-layout": "vertical",
  "ai.enabled": "true",
  "work.enabled": "true",
  "ui.language": "system",
  "ui.density": "comfortable",
  "ui.text-size": "default",
  "ui.contrast": "false",
  "search.history": "true",
  "tabs.new-position": "end",
  "tabs.after-close": "next",
  "tabs.switch-to-open": "true",
  "tabs.startup": "continue",
} as const;
export type PreferenceKey = keyof typeof defaults;
const values = $state<Record<PreferenceKey, string>>({ ...defaults });
let pending = $state<PreferenceKey | null>(null);
let failed = $state(false);
let epoch = 0;
let stop: (() => void) | null = null;
let initialized = false;
let initializing: Promise<void> | null = null;
let lifetime: AbortController | null = null;
const versions: Partial<Record<PreferenceKey, number>> = {};
export const value = (key: PreferenceKey) => values[key];
export const saving = () => pending !== null;
export const saveFailed = () => failed;
function apply(key: PreferenceKey, next: string) {
  values[key] = next;
  versions[key] = (versions[key] ?? 0) + 1;
}
async function initialize(generation: number, signal: AbortSignal) {
  const updated: Partial<Record<PreferenceKey, true>> = {};
  const unsubscribe = await events.uiCommand.listen(({ payload }) => {
    if (generation !== epoch) return;
    if (payload.startsWith("theme.")) {
      updated.appearance = true;
      apply("appearance", payload.slice(6));
    }
    if (!payload.startsWith("preference.")) return;
    const [key, next] = payload.slice(11).split("=");
    if (key === undefined || !Object.hasOwn(defaults, key) || next === undefined) return;
    updated[key as PreferenceKey] = true;
    apply(key as PreferenceKey, next);
  });
  if (generation !== epoch) {
    unsubscribe();
    return;
  }
  stop = unsubscribe;
  await Promise.all(
    (Object.keys(defaults) as PreferenceKey[]).map(async (key) => {
      try {
        const stored = await observe(commands.settingGet(key), 2000, signal);
        if (generation === epoch && !updated[key] && stored.state === "received")
          apply(key, stored.value ?? defaults[key]);
      } catch {
        /* Retain safe defaults if a read is unavailable. */
      }
    }),
  );
  if (generation === epoch) initialized = true;
}

export function init(): Promise<void> {
  if (initializing) return initializing;
  if (initialized) return Promise.resolve();
  lifetime = new AbortController();
  const generation = ++epoch;
  const task = initialize(generation, lifetime.signal);
  initializing = task;
  void task.then(
    () => {
      if (initializing === task) initializing = null;
    },
    () => {
      if (generation === epoch) {
        stop?.();
        stop = null;
        lifetime?.abort();
      }
      if (initializing === task) initializing = null;
    },
  );
  return task;
}
export function dispose() {
  epoch++;
  lifetime?.abort();
  lifetime = null;
  initializing = null;
  initialized = false;
  stop?.();
  stop = null;
  pending = null;
  failed = false;
  for (const key of Object.keys(defaults) as PreferenceKey[]) {
    values[key] = defaults[key];
    delete versions[key];
  }
}
export async function set(key: PreferenceKey, next: string) {
  if (!initialized) {
    failed = true;
    return;
  }
  if (pending !== null || values[key] === next) return;
  const generation = epoch;
  const signal = lifetime?.signal;
  pending = key;
  failed = false;
  try {
    const result = await settle(commands.settingSet(key, next), 5000, signal);
    if (result.outcome === "failed" || result.outcome === "rejected")
      throw new Error("not processed");
    // Queue admission is not durable success. Re-read through the store before
    // ending the pending presentation; never display a fabricated Saved toast.
    let observed = false;
    const deadline = performance.now() + 1500;
    while (generation === epoch && !signal?.aborted) {
      const remaining = deadline - performance.now();
      if (remaining <= 0) break;
      const version = versions[key];
      const stored = await observe(commands.settingGet(key), remaining, signal);
      if (generation !== epoch || signal?.aborted) return;
      if (stored.state === "received" && stored.value === next) {
        observed = true;
        if (versions[key] === version) apply(key, next);
        break;
      }
      if (stored.state === "timeout" || stored.state === "aborted") break;
      await pause(Math.min(100, Math.max(0, deadline - performance.now())), signal);
    }
    if (!observed) throw new Error("readback unavailable");
  } catch {
    if (generation === epoch) failed = true;
  } finally {
    if (generation === epoch) pending = null;
  }
}

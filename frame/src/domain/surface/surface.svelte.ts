import { listenAll } from "$shared/lib/lifecycle";
import { flushSync } from "svelte";
import { commands } from "$shared/ipc/bindings";
import { settle } from "$domain/operations";
import { events } from "$shared/ipc/native-events";

export type BrowserPage = "settings" | "history" | "downloads" | "work";
let page = $state<BrowserPage | null>(null);
let error = $state(false);
let generation = 0;
let navigationEpoch = 0;
let stop: (() => void) | null = null;
let observationLifetime = new AbortController();
let initialized = false;
let initializing: Promise<void> | null = null;
export const currentPage = () => page;
export const navigationFailed = () => error;
async function initialize(epoch: number) {
  const listeners = await listenAll([
    events.browserReturn.listen(() => {
      if (epoch === generation)
        flushSync(() => {
          page = null;
        });
    }),
    events.uiCommand.listen(({ payload }) => {
      if (epoch !== generation) return;
      if (payload === "browser.return-failed") {
        navigationEpoch++;
        error = true;
        return;
      }
      const confirmed =
        payload === "browser.return"
          ? null
          : payload === "browser.work"
            ? "work"
            : payload === "browser.settings"
              ? "settings"
              : payload === "browser.history"
                ? "history"
                : payload === "browser.downloads"
                  ? "downloads"
                  : undefined;
      if (confirmed !== undefined) {
        navigationEpoch++;
        flushSync(() => {
          error = false;
          page = confirmed;
        });
      }
    }),
  ]);
  if (epoch !== generation) {
    for (const unsubscribe of listeners) unsubscribe();
  } else {
    initialized = true;
    stop = () => {
      for (const unsubscribe of listeners) unsubscribe();
    };
  }
}
export function init(): Promise<void> {
  if (initializing) return initializing;
  if (initialized) return Promise.resolve();
  if (observationLifetime.signal.aborted) observationLifetime = new AbortController();
  const task = initialize(++generation);
  initializing = task;
  void task.then(
    () => {
      if (initializing === task) initializing = null;
    },
    () => {
      if (initializing === task) initializing = null;
    },
  );
  return task;
}
export function dispose() {
  generation++;
  observationLifetime.abort();
  initialized = false;
  initializing = null;
  navigationEpoch++;
  stop?.();
  stop = null;
  page = null;
  error = false;
}
export async function open(next: BrowserPage | null) {
  const request = ++navigationEpoch;
  const fail = () => {
    if (request === navigationEpoch) error = true;
  };
  error = false;
  try {
    const result = await settle(
      commands.runCommand(`browser.${next ?? "return"}`),
      5000,
      observationLifetime.signal,
    );
    if (result.outcome === "failed" || result.outcome === "rejected") fail();
  } catch {
    fail();
  }
}

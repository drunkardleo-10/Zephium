/**
 * Bringing bookmarks and history over from another browser, and becoming the
 * default browser. Native owns both; this module is the contract chrome draws
 * against. Until native provides an adapter the port reports itself
 * unavailable, and nothing is claimed that did not happen.
 */

export type ImportKind = "bookmarks" | "history";

type ImportProfile = { id: string; name: string };

type ImportSource = {
  /** Stable for the life of the process; passed back to `start`. */
  id: string;
  /** The browser's family, which picks its mark: chrome, arc, safari... */
  browser: string;
  /** As the browser names itself, for display. */
  name: string;
  profiles: ImportProfile[];
  /** What this source can supply at all. */
  kinds: ImportKind[];
  /** The platform must grant access first (Safari's Full Disk Access). */
  needsPermission: boolean;
  /** The browser holds its files open and has to be quit first. */
  running: boolean;
};

type KindState = "queued" | "running" | "done" | "failed" | "skipped";

type KindProgress = {
  kind: ImportKind;
  state: KindState;
  done: number;
  /** Unknown until the source has been counted. */
  total: number | null;
};

export type ImportJob = {
  source: string;
  profile: string;
  kinds: KindProgress[];
  finished: boolean;
  cancelled: boolean;
};

export type DefaultBrowserStatus = {
  isDefault: boolean;
  /** Whether asking is possible here; Windows can only open its Settings. */
  canRequest: boolean;
};

/** What native provides. Progress arrives through `onProgress` as whole
 *  snapshots of the job, newest last. */
export type ImportAdapter = {
  sources(): Promise<ImportSource[]>;
  start(source: string, profile: string, kinds: ImportKind[]): Promise<boolean>;
  cancel(): Promise<void>;
  onProgress(listener: (job: ImportJob) => void): () => void;
  openPermissionSettings(source: string): Promise<void>;
  defaultBrowser(): Promise<DefaultBrowserStatus>;
  requestDefault(): Promise<void>;
};

let adapter = $state.raw<ImportAdapter | null>(null);
let sources = $state.raw<ImportSource[] | null>(null);
let job = $state.raw<ImportJob | null>(null);
let starting = $state(false);
let defaultStatus = $state.raw<DefaultBrowserStatus | null>(null);
let stop: (() => void) | null = null;
let generation = 0;

/** Whether native can import at all in this build. */
export const available = () => adapter !== null;
/** Browsers found on this device, or null while still looking. */
export const found = () => sources;
export const current = () => job;
export const busy = () => starting || (job !== null && !job.finished);
export const defaultBrowser = () => defaultStatus;

/** Native installs itself here once it can import; null withdraws it. */
export function provide(next: ImportAdapter | null) {
  reset();
  adapter = next;
}

export async function detect() {
  const port = adapter;
  if (!port) return;
  const epoch = ++generation;
  // One listener for the adapter's life; reset() removes it with the adapter.
  stop ??= port.onProgress((next) => {
    if (adapter === port) job = next;
  });
  const [listed, status] = await Promise.allSettled([port.sources(), port.defaultBrowser()]);
  if (epoch !== generation) return;
  sources = listed.status === "fulfilled" ? listed.value : [];
  defaultStatus = status.status === "fulfilled" ? status.value : null;
}

export async function start(source: string, profile: string, kinds: ImportKind[]) {
  const port = adapter;
  if (!port || busy() || kinds.length === 0) return false;
  starting = true;
  try {
    return await port.start(source, profile, kinds);
  } catch {
    return false;
  } finally {
    starting = false;
  }
}

export async function cancel() {
  await adapter?.cancel().catch(() => {});
}

export async function openPermissionSettings(source: string) {
  await adapter?.openPermissionSettings(source).catch(() => {});
}

/** Asks the system; the answer is read back rather than assumed, because the
 *  person can decline, and Windows only opens its Settings. */
export async function requestDefault() {
  const port = adapter;
  if (!port) return;
  await port.requestDefault().catch(() => {});
  await refreshDefault();
}

export async function refreshDefault() {
  const port = adapter;
  if (!port) return;
  const epoch = generation;
  try {
    const status = await port.defaultBrowser();
    if (epoch === generation) defaultStatus = status;
  } catch {
    /* The last known answer stands. */
  }
}

function reset() {
  generation++;
  stop?.();
  stop = null;
  sources = null;
  job = null;
  starting = false;
  defaultStatus = null;
}

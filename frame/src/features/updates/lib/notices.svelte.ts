/**
 * What the sidebar says about updates. The acknowledgements are kept apart
 * from preferences on purpose: resetting settings must not bring a dismissed
 * notice back.
 */
import { commands } from "$shared/ipc/bindings";
import { runtime } from "$domain/runtime";
import { updates } from "$domain/updates";
import { selectNotices, type Notices } from "./select";

const SEEN = "notice.seen-version";
const SECURITY = "notice.security-dismissed";

let version = $state<string | null>(null);
let seen = $state<string | null>(null);
let securityDismissed = $state<string | null>(null);
let generation = 0;
let started = false;

export const current = (): Notices =>
  selectNotices({
    status: updates.status(),
    relaunching: updates.pendingRelaunch(),
    version,
    seen,
    securityDismissed,
    advisories: runtime.status().security_advisories,
  });

function remember(key: string, value: string) {
  // A write that did not land only means the notice is shown again next launch.
  void commands.settingSet(key, value).catch(() => {});
}

async function read(key: string): Promise<string | null> {
  try {
    return (await commands.settingGet(key)) ?? "";
  } catch {
    return null;
  }
}

/** Reads the running version and what was already acknowledged. */
export async function init(): Promise<void> {
  if (started) return;
  started = true;
  const at = ++generation;
  const [about, storedSeen, storedSecurity] = await Promise.all([
    commands.aboutInfo().catch(() => null),
    read(SEEN),
    read(SECURITY),
  ]);
  if (at !== generation || !about) return;
  version = about.version;
  securityDismissed = storedSecurity;
  // A first run has nothing to announce; it only marks where it started.
  if (storedSeen === "") {
    seen = about.version;
    remember(SEEN, about.version);
  } else {
    seen = storedSeen;
  }
}

export function dispose() {
  started = false;
  generation += 1;
  version = seen = securityDismissed = null;
}

export function acknowledgeUpdate() {
  if (version === null) return;
  seen = version;
  remember(SEEN, version);
}

export function dismissSecurity() {
  if (version === null) return;
  securityDismissed = version;
  remember(SECURITY, version);
}

/**
 * Whether Zephium is the default browser, as the system reports it. Asking is
 * the system's decision (macOS confirms, Windows opens its Settings), so the
 * answer is always read back, never assumed.
 */
import { commands, type DefaultBrowserStatus } from "$shared/ipc/bindings";

let status = $state.raw<DefaultBrowserStatus | null>(null);
let asking = $state(false);
let epoch = 0;

export const current = () => status;
export const pending = () => asking;

export async function refresh() {
  const read = ++epoch;
  try {
    const next = await commands.defaultBrowserStatus();
    if (next && read === epoch) status = next;
  } catch {
    // The last known answer stands.
  }
}

export async function request() {
  if (asking) return;
  asking = true;
  const read = ++epoch;
  try {
    const next = await commands.defaultBrowserRequest();
    if (next && read === epoch) status = next;
  } catch {
    // Nothing changed that chrome can claim.
  } finally {
    asking = false;
  }
}

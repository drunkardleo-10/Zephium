import type { BrowserCredentialCapabilityView } from "$shared/ipc/bindings";
import { commands } from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";

let capability = $state.raw<BrowserCredentialCapabilityView | null>(null);
let requestBusy = $state(false);
let activation = 0;
let stop: (() => void) | null = null;

export function current(): BrowserCredentialCapabilityView | null {
  return capability;
}

export function busy(): boolean {
  return requestBusy;
}

export async function activate(): Promise<void> {
  const token = ++activation;
  stop?.();
  stop = null;
  capability = null;
  requestBusy = false;

  const unsubscribe = await events.browserCredentialCapabilityChanged.listen((event) => {
    if (token !== activation) return;
    capability = event.payload;
    requestBusy = false;
  });
  if (token !== activation) {
    unsubscribe();
    return;
  }
  stop = unsubscribe;
  try {
    const next = await commands.browserCredentialCapability();
    if (token === activation) capability = next;
  } catch {
    if (token === activation) capability = null;
  }
}

export function deactivate(): void {
  activation += 1;
  stop?.();
  stop = null;
  capability = null;
  requestBusy = false;
}

export async function requestPasskeyAuthorization(): Promise<void> {
  if (requestBusy || capability?.can_request_passkey_authorization !== true) return;
  requestBusy = true;
  try {
    if (!(await commands.browserPasskeyAuthorizationRequest())) {
      requestBusy = false;
      capability = await commands.browserCredentialCapability();
    }
  } catch {
    requestBusy = false;
  }
}

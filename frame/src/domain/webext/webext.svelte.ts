import { commands, type WebExtensionReview, type WebExtensionView } from "$shared/ipc/bindings";

let available = $state<boolean | null>(null);
let installed = $state<WebExtensionView[]>([]);
let pendingReview = $state<WebExtensionReview | null>(null);
let preparing = $state(false);
let confirming = $state(false);
let failure = $state<string | null>(null);
let refreshTimer: ReturnType<typeof setTimeout> | undefined;

export const isAvailable = () => available === true;
export const list = () => installed;
export const review = () => pendingReview;
export const isPreparing = () => preparing;
export const isConfirming = () => confirming;
export const error = () => failure;

export async function refresh(): Promise<void> {
  const result = await commands.webExtensionList();
  if (result.status === "ok") {
    available = true;
    installed = result.data;
    // Extensions start in the background after a change; follow them until
    // every one has settled.
    clearTimeout(refreshTimer);
    if (installed.some((extension) => extension.state === "starting")) {
      refreshTimer = setTimeout(() => void refresh(), 600);
    }
  } else if (available === null) {
    available = false;
  }
}

export async function prepare(tabId: string): Promise<void> {
  if (preparing) return;
  preparing = true;
  failure = null;
  const result = await commands.webExtensionPrepare(tabId);
  preparing = false;
  if (result.status === "ok") pendingReview = result.data;
  else failure = result.error;
}

export async function confirm(): Promise<void> {
  const current = pendingReview;
  if (current === null || confirming) return;
  confirming = true;
  const result = await commands.webExtensionConfirm(current.id);
  confirming = false;
  if (result.status === "ok") {
    pendingReview = null;
    failure = null;
    await refresh();
  } else {
    failure = result.error;
  }
}

export function cancel(): void {
  pendingReview = null;
  failure = null;
  void commands.webExtensionCancel();
}

export async function setEnabled(id: string, enabled: boolean): Promise<void> {
  const result = await commands.webExtensionSetEnabled(id, enabled);
  failure = result.status === "ok" ? null : result.error;
  await refresh();
}

export async function uninstall(id: string): Promise<void> {
  const result = await commands.webExtensionUninstall(id);
  failure = result.status === "ok" ? null : result.error;
  await refresh();
}

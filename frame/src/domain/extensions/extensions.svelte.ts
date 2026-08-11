import type {
  ExtensionActionFailedView,
  ExtensionActionFailure,
  ExtensionActionView,
  ExtensionActionsView,
  ExtensionInstallCandidateView,
  ExtensionManagementEntryView,
  ExtensionManagementView,
  OperationAdmission,
  OperationDisposition,
} from "../../shared/ipc/bindings";
import { SvelteSet } from "svelte/reactivity";
import { commands } from "../../shared/ipc/bindings";
import { events } from "../../shared/ipc/native-events";
import * as operations from "../operations/operations";
import {
  ExtensionManagementProjectionModel,
  ExtensionProjectionModel,
  failureForContext,
  initialExtensionManagement,
  managementForProfile,
} from "./extensions-model";

const NOTICE_LIFETIME_MS = 5_000;
const MANAGEMENT_IPC_TIMEOUT_MS = 5_000;
const MANAGEMENT_SETTLEMENT_TIMEOUT_MS = 30_000;

const model = new ExtensionProjectionModel();
const managementModel = new ExtensionManagementProjectionModel();
let state = $state.raw<ExtensionActionsView>(model.actions);
let managementState = $state.raw<ExtensionManagementView>(managementModel.management);
type VisibleFailure = Omit<ExtensionActionFailedView, "projection_revision"> & {
  projectionRevision?: string;
};
let failure = $state.raw<VisibleFailure | null>(null);
const invoking = new SvelteSet<string>();
type ManagementMutation = {
  subject: string;
  kind: "install" | "enable" | "disable" | "uninstall";
};
let managementMutation = $state.raw<ManagementMutation | null>(null);
let managementNotice = $state<string | null>(null);
let managementVisible = false;

let lifecycle = 0;
let initialized = false;
let initializing: Promise<void> | null = null;
let unlisten: (() => void) | null = null;
let noticeTimer: ReturnType<typeof setTimeout> | null = null;

type Unlisten = () => void;

export const snapshot = () => state;
export const failureReason = (profileId: string | null, tabId: string | null) =>
  failureForContext(failure, profileId, tabId);
export const isInvoking = (installId: string) => invoking.has(installId);
export const management = (profileId: string | null) =>
  managementForProfile(managementState, profileId);
export const activeManagementMutation = () => managementMutation;
export const managementFailure = () => managementNotice;

export function activeActions(profileId: string | null, tabId: string | null) {
  return profileId !== null &&
    tabId !== null &&
    state.profile_id === profileId &&
    state.tab_id === tabId
    ? state.actions
    : [];
}

function clearVisibleFailure() {
  if (noticeTimer !== null) clearTimeout(noticeTimer);
  noticeTimer = null;
  failure = null;
}

function publishFailure(
  reason: ExtensionActionFailure,
  profileId: string,
  tabId: string,
  revision?: string,
) {
  failure = { reason, profile_id: profileId, tab_id: tabId, projectionRevision: revision };
  if (noticeTimer !== null) clearTimeout(noticeTimer);
  const generation = lifecycle;
  noticeTimer = setTimeout(() => {
    noticeTimer = null;
    if (generation !== lifecycle) return;
    if (revision !== undefined) model.clearFailure(revision);
    failure = null;
  }, NOTICE_LIFETIME_MS);
}

async function resolveListeners(promises: readonly Promise<Unlisten>[]): Promise<Unlisten[]> {
  const results = await Promise.allSettled(promises);
  const listeners: Unlisten[] = [];
  let failed = false;
  let failureReason: unknown;
  for (const result of results) {
    if (result.status === "fulfilled") listeners.push(result.value);
    else if (!failed) {
      failed = true;
      failureReason = result.reason;
    }
  }
  if (failed) {
    for (const stop of listeners) stop();
    throw failureReason;
  }
  return listeners;
}

async function boundedIpc<T>(request: Promise<T>, milliseconds: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      request,
      new Promise<T>((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error("bounded privileged IPC timeout")), milliseconds);
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

async function initialize(generation: number) {
  const listeners = await resolveListeners([
    events.extensionActionsChanged.listen((event) => {
      if (generation !== lifecycle || !model.applyActions(event.payload)) return;
      state = model.actions;
      clearVisibleFailure();
    }),
    events.extensionActionFailed.listen((event) => {
      if (generation !== lifecycle || !model.applyFailure(event.payload)) return;
      publishFailure(
        event.payload.reason,
        event.payload.profile_id,
        event.payload.tab_id,
        event.payload.projection_revision,
      );
    }),
    events.extensionManagementChanged.listen((event) => {
      if (generation !== lifecycle || !managementModel.apply(event.payload)) return;
      managementState = managementModel.management;
    }),
  ]);
  if (generation !== lifecycle) {
    for (const stop of listeners) stop();
    return;
  }
  unlisten = () => {
    for (const stop of listeners) stop();
  };
  initialized = true;
}

export function init(): Promise<void> {
  if (initialized) return Promise.resolve();
  if (initializing !== null) return initializing;
  const generation = ++lifecycle;
  const task = initialize(generation);
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
  lifecycle += 1;
  initialized = false;
  initializing = null;
  unlisten?.();
  unlisten = null;
  if (noticeTimer !== null) clearTimeout(noticeTimer);
  noticeTimer = null;
  invoking.clear();
  managementMutation = null;
  managementNotice = null;
  failure = null;
  if (managementVisible) void commands.extensionManagementSetVisible(false).catch(() => {});
  managementVisible = false;
}

export async function invoke(
  profileId: string,
  tabId: string,
  action: ExtensionActionView,
  anchor: DOMRect,
): Promise<void> {
  const key = action.runtime.install_id;
  if (!action.enabled || invoking.has(key)) return;
  invoking.add(key);
  try {
    const admission = await commands.extensionActionInvoke(
      profileId,
      action.runtime.install_id,
      action.runtime.generation,
      action.revision,
      anchor.left,
      anchor.top,
      anchor.width,
      anchor.height,
    );
    if (!admission.accepted) publishFailure("invalid_request", profileId, tabId);
  } catch {
    publishFailure("native_admission_failed", profileId, tabId);
  } finally {
    invoking.delete(key);
  }
}

export async function setManagementVisible(visible: boolean): Promise<boolean> {
  managementNotice = null;
  // Selectors from the previous subscription generation are never rendered
  // while a new visibility command is in flight (or after close).
  managementState = initialExtensionManagement();
  try {
    const accepted = await boundedIpc(
      commands.extensionManagementSetVisible(visible),
      MANAGEMENT_IPC_TIMEOUT_MS,
    );
    managementVisible = visible && accepted;
    if (!accepted && visible) managementNotice = "Extension management is unavailable.";
    return accepted;
  } catch {
    managementVisible = false;
    if (visible) managementNotice = "Zephium couldn't load extensions.";
    return false;
  }
}

function managementDispositionMessage(disposition: OperationDisposition): string | null {
  if (disposition.outcome === "applied" || disposition.outcome === "no_op") {
    if (disposition.reason === "extension_activation_pending") {
      return "The extension is enabled and will activate when its runtime becomes available.";
    }
    if (disposition.reason === "extension_enablement_pending") {
      return "The extension was installed, but enabling it is still pending.";
    }
    return null;
  }
  switch (disposition.reason) {
    case "store_conflict":
      return "Extensions changed. Review the refreshed list and try again.";
    case "store_outcome_unknown":
    case "store_reconciliation_failed":
      return "Zephium couldn't verify the extension change. Restart before trying again.";
    case "store_admission_rejected":
      return "Extension management is temporarily unavailable.";
    case "invalid_scope":
      return "This extension can no longer be changed from the current profile.";
    default:
      return "Zephium couldn't apply the extension change.";
  }
}

async function settleManagementMutation(
  mutation: ManagementMutation,
  dispatch: () => Promise<OperationAdmission>,
): Promise<void> {
  if (managementMutation !== null) return;
  managementMutation = mutation;
  managementNotice = null;
  try {
    try {
      await operations.init();
    } catch {
      // The bounded desktop ledger remains available even if event listener
      // registration races this trusted-UI invocation.
    }
    const admission = await boundedIpc(dispatch(), MANAGEMENT_IPC_TIMEOUT_MS);
    if (!admission.accepted || admission.operation_id === null) {
      managementNotice = "The extension list changed. Refresh it and try again.";
      return;
    }
    // The admitted write consumes this exact compare-and-swap cohort. Keep
    // controls absent until Shell publishes its post-settlement replacement.
    managementState = initialExtensionManagement();
    const resolution = await operations.waitForDisposition(
      admission.operation_id,
      MANAGEMENT_SETTLEMENT_TIMEOUT_MS,
    );
    if (resolution.state === "processed") {
      managementNotice = managementDispositionMessage(resolution.disposition);
    } else if (resolution.state === "pending") {
      managementNotice = "The extension change is still pending.";
    } else {
      managementNotice = "Zephium couldn't verify the extension change.";
    }
  } catch {
    managementNotice = "Extension management is temporarily unavailable.";
  } finally {
    managementMutation = null;
  }
}

export function setEnabled(
  entry: ExtensionManagementEntryView,
  catalogRevision: string,
  enabled: boolean,
): void {
  void settleManagementMutation(
    { subject: entry.install_id, kind: enabled ? "enable" : "disable" },
    () =>
      commands.extensionManagementSetEnabled(
        entry.install_id,
        catalogRevision,
        entry.install_revision,
        enabled,
      ),
  );
}

export function uninstall(entry: ExtensionManagementEntryView, catalogRevision: string): void {
  void settleManagementMutation({ subject: entry.install_id, kind: "uninstall" }, () =>
    commands.extensionManagementUninstall(
      entry.install_id,
      catalogRevision,
      entry.install_revision,
    ),
  );
}

export function install(
  candidate: ExtensionInstallCandidateView,
  catalogRevision: string,
  fileAccess: boolean,
  privateAccess: boolean,
): void {
  void settleManagementMutation(
    { subject: String(candidate.candidate_index), kind: "install" },
    () =>
      commands.extensionManagementInstall(
        candidate.candidate_index,
        catalogRevision,
        fileAccess,
        privateAccess,
      ),
  );
}

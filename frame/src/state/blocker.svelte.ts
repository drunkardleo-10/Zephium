import type { BlockerStatusView, OperationAdmission } from "../ipc/bindings";
import { commands } from "../ipc/bindings";
import { events } from "../ipc/native-events";
import { BLOCKER_ZERO_REVISION, initialBlockerStatus, newestBlockerStatus } from "./blocker-model";
import * as operations from "./operations";

let state = $state.raw<BlockerStatusView>(initialBlockerStatus());

const RECONCILE_ATTEMPTS = 3;
const RECONCILE_RETRY_MS = 500;
const IPC_ATTEMPT_TIMEOUT_MS = 2_000;
const MUTATION_ADMISSION_TIMEOUT_MS = 5_000;

let lifecycle = 0;
let initialized = false;
let initializing: Promise<void> | null = null;
let reconciling: Promise<void> | null = null;
let unlisten: (() => void) | null = null;

export const status = () => state;

function delay(milliseconds: number) {
  return new Promise<void>((resolve) => setTimeout(resolve, milliseconds));
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

function apply(candidate: BlockerStatusView) {
  const newest = newestBlockerStatus(state, candidate);
  if (newest !== state) state = newest;
}

async function reconcileStatus(generation: number) {
  for (let attempt = 0; attempt < RECONCILE_ATTEMPTS && generation === lifecycle; attempt += 1) {
    try {
      const response = await boundedIpc(commands.blockerStatus(), IPC_ATTEMPT_TIMEOUT_MS);
      if (generation !== lifecycle) return;
      if (response.status !== "ok") {
        await delay(RECONCILE_RETRY_MS);
        continue;
      }
      const result = response.data;
      apply(result);
      if (result.projection_revision !== BLOCKER_ZERO_REVISION) return;
    } catch {
      // Setup/navigation races are bounded and reconciled by the scoped event
      // listener. Never turn diagnostics failure into browser startup failure.
    }
    await delay(RECONCILE_RETRY_MS);
  }
}

function beginReconciliation(generation: number): Promise<void> {
  if (reconciling !== null) return reconciling;
  const task = reconcileStatus(generation);
  reconciling = task;
  void task.then(
    () => {
      if (reconciling === task) reconciling = null;
    },
    () => {
      if (reconciling === task) reconciling = null;
    },
  );
  return task;
}

async function initialize(generation: number) {
  let stop: () => void;
  try {
    stop = await events.blockerStatusChanged.listen((event) => {
      if (generation === lifecycle) apply(event.payload);
    });
  } catch {
    // The bootstrap query still gives the UI an actor-ordered snapshot. Keep
    // `initialized` false so opening the popover retries event registration.
    await beginReconciliation(generation);
    return;
  }
  if (generation !== lifecycle) {
    stop();
    return;
  }
  unlisten = stop;
  initialized = true;
  await beginReconciliation(generation);
}

export function init(): Promise<void> {
  if (initializing !== null) return initializing;
  if (initialized) return Promise.resolve();

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

export async function refresh(): Promise<void> {
  if (!initialized) {
    await init();
    return;
  }

  const generation = lifecycle;
  const active = reconciling;
  if (active !== null) await active;
  if (generation !== lifecycle || !initialized) return;
  await beginReconciliation(generation);
}

export type BlockerMutationResult =
  operations.OperationResolution | { state: "not_admitted" } | { state: "unavailable" };

async function settleMutation(
  dispatch: () => Promise<OperationAdmission>,
): Promise<BlockerMutationResult> {
  // Install the operation event listener before admission. The ledger query
  // remains an exact fallback if initialization races trusted UI startup.
  try {
    await operations.init();
  } catch {
    // Do not reject an otherwise valid mutation solely because diagnostics
    // event registration failed; waitForDisposition also queries the ledger.
  }

  let result: BlockerMutationResult;
  try {
    const admission = await boundedIpc(dispatch(), MUTATION_ADMISSION_TIMEOUT_MS);
    if (!admission.accepted) {
      result = { state: "not_admitted" };
    } else if (admission.operation_id === null) {
      // Blocker mutations are actor-owned and must always carry an id. Treat a
      // contradictory admission as unavailable rather than inventing success.
      result = { state: "unavailable" };
    } else {
      result = await operations.waitForDisposition(admission.operation_id);
    }
  } catch {
    result = { state: "unavailable" };
  }

  // Query after terminal settlement (or the bounded wait) so the diagnostic
  // view is based on an actor-ordered projection, not optimistic UI state.
  await refresh();
  return result;
}

export const setEnabled = (enabled: boolean) =>
  settleMutation(() => commands.blockerSetEnabled(enabled));

export const retry = (failedGeneration: string) =>
  settleMutation(() => commands.blockerRetry(failedGeneration));

export const refreshSources = () => settleMutation(() => commands.blockerRefreshSources());

export function dispose() {
  if (!initialized && initializing === null && reconciling === null && unlisten === null) {
    return;
  }

  lifecycle += 1;
  initialized = false;
  initializing = null;
  reconciling = null;
  unlisten?.();
  unlisten = null;
}

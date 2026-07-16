import type { OperationDisposition } from "../ipc/bindings";
import { commands } from "../ipc/bindings";
import { events } from "../ipc/native-events";

// Rust retains every accepted operation in its bounded process-local ledger
// until this privileged WebView acknowledges the actor's disposition. Keep a
// smaller recent-result window here for UI consumers, but acknowledgement is
// independent of that presentation cache and is retried after transient IPC
// failures.
const MAX_RECENT_RESULTS = 256;
const RECONCILE_ATTEMPTS = 20;
const RECONCILE_RETRY_MS = 250;
const ACK_RETRY_MS = 1_000;

const recent = new Map<string, OperationDisposition>();
const pendingAcks = new Set<string>();

let lifecycle = 0;
let initialized = false;
let initializing: Promise<void> | null = null;
let unlisten: (() => void) | null = null;
let ackTimer: ReturnType<typeof setTimeout> | null = null;

function delay(milliseconds: number) {
  return new Promise<void>((resolve) => setTimeout(resolve, milliseconds));
}

function remember(disposition: OperationDisposition) {
  if (!recent.has(disposition.operation_id)) {
    recent.set(disposition.operation_id, disposition);
    if (recent.size > MAX_RECENT_RESULTS) {
      const oldest = recent.keys().next().value;
      if (oldest !== undefined) recent.delete(oldest);
    }
  }
  pendingAcks.add(disposition.operation_id);
  scheduleAcknowledgements(0);
}

function scheduleAcknowledgements(after: number) {
  if (ackTimer !== null || pendingAcks.size === 0) return;
  ackTimer = setTimeout(() => {
    ackTimer = null;
    void flushAcknowledgements();
  }, after);
}

async function flushAcknowledgements() {
  for (const operationId of [...pendingAcks]) {
    try {
      // A successful false means another delivery path already acknowledged
      // the result (or the process-local ledger was replaced). Either way,
      // there is no server entry left for this client to retain.
      await commands.operationAcknowledge(operationId);
      pendingAcks.delete(operationId);
    } catch {
      // Keep the id and retry. Never turn a temporary IPC failure into an
      // ever-growing native ledger or a duplicate user-visible disposition.
    }
  }
  scheduleAcknowledgements(ACK_RETRY_MS);
}

async function reconcile(generation: number) {
  for (let attempt = 0; attempt < RECONCILE_ATTEMPTS && generation === lifecycle; attempt++) {
    try {
      const dispositions = await commands.operationsReconcile();
      if (generation !== lifecycle) return;
      for (const disposition of dispositions) remember(disposition);
      return;
    } catch {
      await delay(RECONCILE_RETRY_MS);
    }
  }
}

async function initialize(generation: number) {
  const stop = await events.operationProcessed.listen((event) => {
    if (generation === lifecycle) remember(event.payload);
  });
  if (generation !== lifecycle) {
    stop();
    return;
  }
  unlisten = stop;
  initialized = true;
  await reconcile(generation);
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
  if (ackTimer !== null) clearTimeout(ackTimer);
  ackTimer = null;
}

export const result = (operationId: string) => recent.get(operationId);

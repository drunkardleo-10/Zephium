import type {
  OperationDisposition,
  PagePermissionPromptDecisionInput,
  PagePermissionPromptEntryView,
  PagePermissionPromptView,
} from "../../shared/ipc/bindings";
import { commands } from "../../shared/ipc/bindings";
import { events } from "../../shared/ipc/native-events";
import * as operations from "../operations/operations";
import {
  PagePermissionPromptProjectionModel,
  initialPagePermissionPrompt,
} from "./page-permissions-model";

const IPC_TIMEOUT_MS = 5_000;
const SETTLEMENT_TIMEOUT_MS = 30_000;

const model = new PagePermissionPromptProjectionModel();
let state = $state.raw<PagePermissionPromptView>(model.view);
let responding = $state(false);
let notice = $state<string | null>(null);
let lifecycle = 0;
let initialized = false;
let initializing: Promise<void> | null = null;
let unlisten: (() => void) | null = null;

export const prompt = () => state.prompt;
export const busy = () => responding || state.prompt?.processing === true;
export const failure = () => notice;

function samePrompt(
  left: PagePermissionPromptEntryView | null,
  right: PagePermissionPromptEntryView,
): boolean {
  return (
    left?.profile_id === right.profile_id &&
    left.item_id === right.item_id &&
    left.request_id === right.request_id
  );
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

function dispositionMessage(disposition: OperationDisposition): string | null {
  if (disposition.outcome === "applied" || disposition.outcome === "no_op") return null;
  switch (disposition.reason) {
    case "invalid_scope":
      return "This request is no longer available.";
    case "store_conflict":
      return "This site's saved permission changed. Review the request and try again.";
    case "store_outcome_unknown":
    case "store_reconciliation_failed":
      return "Zephium couldn't safely verify the saved permission.";
    case "store_admission_rejected":
      return "Saved site permissions are temporarily unavailable.";
    default:
      return "Zephium couldn't apply this permission decision.";
  }
}

async function initialize(generation: number) {
  const stop = await events.pagePermissionPromptChanged.listen((event) => {
    if (generation !== lifecycle || !model.apply(event.payload)) return;
    state = model.view;
    responding = false;
    notice = null;
  });
  if (generation !== lifecycle) {
    stop();
    return;
  }
  unlisten = stop;
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
  state = initialPagePermissionPrompt();
  responding = false;
  notice = null;
}

async function settle(
  retained: PagePermissionPromptEntryView,
  decision: PagePermissionPromptDecisionInput,
): Promise<void> {
  if (!samePrompt(state.prompt, retained) || busy()) return;
  responding = true;
  notice = null;
  try {
    try {
      await operations.init();
    } catch {
      // The operation ledger remains the bounded reconciliation path if its
      // listener races this foreground gesture.
    }
    const admission = await boundedIpc(
      commands.pagePermissionRespond(
        retained.profile_id,
        retained.item_id,
        retained.request_id,
        decision,
      ),
      IPC_TIMEOUT_MS,
    );
    if (!admission.accepted || admission.operation_id === null) {
      notice = "This request is no longer available.";
      responding = false;
      return;
    }
    const resolution = await operations.waitForDisposition(
      admission.operation_id,
      SETTLEMENT_TIMEOUT_MS,
    );
    if (!samePrompt(state.prompt, retained)) return;
    if (resolution.state === "processed") {
      notice = dispositionMessage(resolution.disposition);
      if (notice !== null) responding = false;
    } else if (resolution.state === "pending") {
      notice = "Zephium is still applying the saved permission.";
    } else {
      notice = "Zephium couldn't verify this permission decision.";
      responding = false;
    }
  } catch {
    if (samePrompt(state.prompt, retained)) {
      notice = "Site permissions are temporarily unavailable.";
      responding = false;
    }
  }
}

export function respond(
  retained: PagePermissionPromptEntryView,
  decision: PagePermissionPromptDecisionInput,
): void {
  void settle(retained, decision);
}

import type { DividerView } from "../../shared/ipc/bindings";
import { commands } from "../../shared/ipc/bindings";
import { events } from "../../shared/ipc/native-events";

let dividerState = $state.raw<DividerView[]>([]);

export const dividers = () => dividerState;

let lifecycle = 0;
let initialized = false;
let initializing: Promise<void> | null = null;
let unlisten: (() => void) | null = null;

async function initialize(generation: number) {
  const stop = await events.layoutChanged.listen((event) => {
    if (generation === lifecycle) dividerState = event.payload.dividers;
  });

  if (generation !== lifecycle) {
    stop();
    return;
  }

  unlisten = stop;
  initialized = true;
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

export function dispose() {
  if (!initialized && initializing === null && unlisten === null) return;

  lifecycle += 1;
  initialized = false;
  initializing = null;
  unlisten?.();
  unlisten = null;
}

export const grab = (x: number, y: number) => void commands.dividerGrab(x, y);
export const drag = (x: number, y: number) => void commands.dividerDrag(x, y);
export const release = (x: number | null, y: number | null) => void commands.dividerRelease(x, y);

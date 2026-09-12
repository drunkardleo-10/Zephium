import { events } from "$shared/ipc/native-events";

type UiCommandState = { id: string; seq: number };

let command = $state.raw<UiCommandState>({ id: "", seq: 0 });
let seq = 0;

export const uiCommand = () => command;

let lifecycle = 0;
let initialized = false;
let initializing: Promise<void> | null = null;
let unlisten: (() => void) | null = null;

async function initialize(generation: number) {
  const stop = await events.uiCommand.listen((event) => {
    if (generation !== lifecycle) return;
    seq += 1;
    command = { id: event.payload, seq };
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

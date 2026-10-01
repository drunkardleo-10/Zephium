import { createLifecycle } from "$shared/lib/lifecycle";
import type { DividerView, WorkPaneLayout } from "$shared/ipc/bindings";
import { commands } from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";

/** The pane's applied native hole with finite geometry; the wire type admits null for NaN. */
export type WorkPaneHole = {
  tab: string;
  x: number;
  y: number;
  width: number;
  height: number;
  presented: boolean;
  generation: number;
};

function hole(pane: WorkPaneLayout | null): WorkPaneHole | null {
  if (!pane || pane.x === null || pane.y === null || pane.width === null || pane.height === null)
    return null;
  return {
    tab: pane.tab,
    x: pane.x,
    y: pane.y,
    width: pane.width,
    height: pane.height,
    presented: pane.presented,
    generation: pane.generation,
  };
}

let dividerState = $state.raw<DividerView[]>([]);
let workPaneState = $state.raw<WorkPaneHole | null>(null);

export const dividers = () => dividerState;
/** The applied native hole of the Work browser pane, or null while no pane is shown. */
export const workPane = () => workPaneState;

const lifecycle = createLifecycle();
let initialized = false;
let initializing: Promise<void> | null = null;
let unlisten: (() => void) | null = null;

async function initialize(generation: number) {
  const stop = await events.layoutChanged.listen((event) => {
    if (!lifecycle.isCurrent(generation)) return;
    dividerState = event.payload.dividers;
    workPaneState = hole(event.payload.work_pane);
  });

  if (!lifecycle.isCurrent(generation)) {
    stop();
    return;
  }

  unlisten = stop;
  initialized = true;
}

export function init(): Promise<void> {
  if (initializing !== null) return initializing;
  if (initialized) return Promise.resolve();

  const generation = lifecycle.begin();
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

  lifecycle.end();
  initialized = false;
  initializing = null;
  unlisten?.();
  unlisten = null;
  workPaneState = null;
}

export const grab = (x: number, y: number) => void commands.dividerGrab(x, y);
export const drag = (x: number, y: number) => void commands.dividerDrag(x, y);
export const release = (x: number | null, y: number | null) => void commands.dividerRelease(x, y);

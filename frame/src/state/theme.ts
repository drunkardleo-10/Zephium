import { commands } from "../ipc/bindings";
import { events } from "../ipc/native-events";

export type Appearance = "system" | "light" | "dark";

const media = window.matchMedia("(prefers-color-scheme: light)");
let appearance: Appearance = "system";
let lifecycle = 0;
let initialized = false;
let initializing: Promise<void> | null = null;
let unlisten: (() => void) | null = null;

function apply() {
  const light = appearance === "light" || (appearance === "system" && media.matches);
  document.documentElement.setAttribute("data-theme", light ? "light" : "dark");
}

async function initialize(generation: number) {
  // Establish system dark/light synchronously before the first native IPC
  // await so the hidden bootstrap document already has deterministic tokens.
  apply();
  let commandAppearance: Appearance | null = null;
  const listener = events.uiCommand.listen((e) => {
    if (generation !== lifecycle) return;
    const id = e.payload;
    const next = id.slice("theme.".length);
    if (!id.startsWith("theme.") || (next !== "light" && next !== "dark" && next !== "system")) {
      return;
    }
    commandAppearance = next;
    appearance = next;
    apply();
  });

  try {
    const info = await commands.uiInfo();
    if (generation !== lifecycle) {
      (await listener)();
      return;
    }
    const kind = !info.material
      ? "none"
      : navigator.userAgent.includes("Mac")
        ? "vibrancy"
        : "acrylic";
    document.documentElement.setAttribute("data-material", kind);

    const stored = await commands.settingGet("appearance");
    if (generation !== lifecycle) {
      (await listener)();
      return;
    }
    if (
      commandAppearance === null &&
      (stored === "light" || stored === "dark" || stored === "system")
    ) {
      appearance = stored;
    }
    apply();

    const stop = await listener;
    if (generation !== lifecycle) {
      stop();
      return;
    }
    media.addEventListener("change", apply);
    unlisten = stop;
    initialized = true;
  } catch (error) {
    const stop = await listener.catch(() => null);
    stop?.();
    throw error;
  }
}

export function init(): Promise<void> {
  if (initializing !== null) return initializing;
  if (initialized) {
    apply();
    return Promise.resolve();
  }

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
  media.removeEventListener("change", apply);
  unlisten?.();
  unlisten = null;
}

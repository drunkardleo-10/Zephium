import { commands } from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";

export type Appearance = "system" | "light" | "dark";

const media = window.matchMedia("(prefers-color-scheme: light)");
let appearance: Appearance = "system";
let lifecycle = 0;
let initialized = false;
let initializing: Promise<void> | null = null;
let unlisten: (() => void) | null = null;

/** Only this module writes appearance axes on the document root. */
export function applyUiPreferences(tint: string, reduceMotion: boolean) {
  document.documentElement.setAttribute("data-tint", tint);
  setReducedMotion(reduceMotion);
}

export function setReducedMotion(reduceMotion: boolean) {
  document.documentElement.setAttribute("data-reduce-motion", String(reduceMotion));
}

export function applyPreview(axes: {
  density: "compact" | "comfortable";
  contrast: boolean;
  text: "large" | "small" | "default";
}) {
  document.documentElement.setAttribute("data-density", axes.density);
  document.documentElement.setAttribute("data-contrast", String(axes.contrast));
  document.documentElement.setAttribute("data-text", axes.text);
}

function apply() {
  const light = appearance === "light" || (appearance === "system" && media.matches);
  document.documentElement.setAttribute("data-theme", light ? "light" : "dark");
}

async function initialize(generation: number) {
  // Establish system dark/light synchronously before the first native IPC
  // await so the hidden bootstrap document already has deterministic tokens.
  apply();
  let commandAppearance: Appearance | null = null;
  let commandMaterial: string | null = null;
  const listener = events.uiCommand.listen((e) => {
    if (generation !== lifecycle) return;
    const id = e.payload;
    if (id.startsWith("material.")) {
      const material = id.slice("material.".length);
      if (["none", "vibrancy", "liquid_glass", "acrylic", "mica"].includes(material)) {
        commandMaterial = material;
        document.documentElement.setAttribute("data-material", material);
      }
      return;
    }
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
    document.documentElement.setAttribute("data-material", commandMaterial ?? info.material);

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

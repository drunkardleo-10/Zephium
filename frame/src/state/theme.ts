import { commands } from "../ipc/bindings";
import { events } from "../ipc/native-events";

export type Appearance = "system" | "light" | "dark";

const media = window.matchMedia("(prefers-color-scheme: light)");
let appearance: Appearance = "system";
let unlisten: (() => void) | null = null;

function apply() {
  const light = appearance === "light" || (appearance === "system" && media.matches);
  document.documentElement.setAttribute("data-theme", light ? "light" : "dark");
}

export async function init() {
  // Establish system dark/light synchronously before the first native IPC
  // await so the hidden bootstrap document already has deterministic tokens.
  apply();
  let commandAppearance: Appearance | null = null;
  const unlistenCommand = events.uiCommand.listen((e) => {
    const id = e.payload;
    if (id.startsWith("theme.")) {
      commandAppearance = id.slice("theme.".length) as Appearance;
      appearance = commandAppearance;
      apply();
    }
  });
  const info = await commands.uiInfo();
  const kind = !info.material
    ? "none"
    : navigator.userAgent.includes("Mac")
      ? "vibrancy"
      : "acrylic";
  document.documentElement.setAttribute("data-material", kind);
  const stored = await commands.settingGet("appearance");
  if (
    commandAppearance === null &&
    (stored === "light" || stored === "dark" || stored === "system")
  ) {
    appearance = stored;
  }
  apply();
  media.addEventListener("change", apply);
  unlisten = await unlistenCommand;
}

export function dispose() {
  media.removeEventListener("change", apply);
  unlisten?.();
  unlisten = null;
}

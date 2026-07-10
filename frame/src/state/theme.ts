import { commands, events } from "../ipc/bindings";

export type Appearance = "system" | "light" | "dark";

const media = window.matchMedia("(prefers-color-scheme: light)");
let appearance: Appearance = "system";
let unlisten: (() => void) | null = null;

function apply() {
  const light = appearance === "light" || (appearance === "system" && media.matches);
  document.documentElement.setAttribute("data-theme", light ? "light" : "dark");
}

export async function init() {
  const info = await commands.uiInfo();
  const kind = !info.material
    ? "none"
    : navigator.userAgent.includes("Mac")
      ? "vibrancy"
      : "acrylic";
  document.documentElement.setAttribute("data-material", kind);
  const stored = await commands.settingGet("appearance");
  if (stored === "light" || stored === "dark" || stored === "system") {
    appearance = stored;
  }
  apply();
  media.addEventListener("change", apply);
  unlisten = await events.uiCommand.listen((e) => {
    const id = e.payload;
    if (id.startsWith("theme.")) {
      appearance = id.slice("theme.".length) as Appearance;
      apply();
    }
  });
}

export function dispose() {
  media.removeEventListener("change", apply);
  unlisten?.();
  unlisten = null;
}

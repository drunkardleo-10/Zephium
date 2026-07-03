import { onCleanup, onMount } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Shell } from "./Shell";
import { Launcher } from "../features/launcher/Launcher";
import { commands } from "../ipc/bindings";
import * as tabs from "../state/tabs";
import * as theme from "../state/theme";
import * as ui from "../state/ui";

// WebKit consumes these inside our own webview before the native menu sees
// them, so the chrome re-routes them onto the shared command entry.
const CHROME_KEYS: Array<[match: (e: KeyboardEvent) => boolean, command: string]> = [
  [(e) => e.ctrlKey && !e.shiftKey && e.key === "Tab", "tab.next"],
  [(e) => e.ctrlKey && e.shiftKey && e.key === "Tab", "tab.previous"],
];

function onKeyDown(e: KeyboardEvent) {
  for (const [match, command] of CHROME_KEYS) {
    if (match(e)) {
      e.preventDefault();
      void commands.runCommand(command);
      return;
    }
  }
}

export default function App() {
  onMount(() => {
    void theme.init();
    onCleanup(() => theme.dispose());
  });

  if (getCurrentWindow().label === "panel") {
    return <Launcher />;
  }

  onMount(() => {
    void tabs.init();
    void ui.init();
    document.addEventListener("keydown", onKeyDown);
    onCleanup(() => {
      tabs.dispose();
      ui.dispose();
      document.removeEventListener("keydown", onKeyDown);
    });
  });
  return <Shell />;
}

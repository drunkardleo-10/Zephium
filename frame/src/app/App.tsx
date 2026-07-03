import { onCleanup, onMount } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Shell } from "./Shell";
import { Launcher } from "../features/launcher/Launcher";
import { commands } from "../ipc/bindings";
import * as tabs from "../state/tabs";
import * as theme from "../state/theme";
import * as ui from "../state/ui";

const IS_MAC = navigator.userAgent.includes("Mac");

// The chrome webview consumes these before any native menu sees them (WebKit
// focus keys on macOS; no menu bar exists at all on Windows/Linux), so the
// chrome re-routes them onto the shared command entry.
const CHROME_KEYS: Array<[match: (e: KeyboardEvent) => boolean, command: string]> = [
  [(e) => e.ctrlKey && !e.shiftKey && e.key === "Tab", "tab.next"],
  [(e) => e.ctrlKey && e.shiftKey && e.key === "Tab", "tab.previous"],
];

if (!IS_MAC) {
  const plain: Array<[string, string]> = [
    ["t", "tab.new"],
    ["w", "tab.close"],
    ["r", "nav.reload"],
    ["l", "url.focus"],
    ["=", "zoom.in"],
    ["-", "zoom.out"],
    ["0", "zoom.reset"],
    ["[", "nav.back"],
    ["]", "nav.forward"],
    [".", "nav.stop"],
  ];
  for (const [key, command] of plain) {
    CHROME_KEYS.push([
      (e) => e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === key,
      command,
    ]);
  }
}

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

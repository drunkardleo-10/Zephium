import { getCurrentWindow } from "@tauri-apps/api/window";
import { onCleanup, onMount, Show } from "solid-js";
import { Launcher } from "../features/launcher/Launcher";
import { Dividers } from "../features/split/Dividers";
import { commands } from "../ipc/bindings";
import * as blocker from "../state/blocker";
import * as layout from "../state/layout";
import * as operations from "../state/operations";
import * as tabs from "../state/tabs";
import * as theme from "../state/theme";
import * as ui from "../state/ui";
import { Shell } from "./Shell";

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
    void (async () => {
      try {
        await theme.init();
        if (getCurrentWindow().label !== "main") return;
        // Hidden native windows may suspend requestAnimationFrame indefinitely.
        // Force style/layout resolution synchronously; the inline bootstrap and
        // default body background stay opaque until material discovery finishes.
        void document.documentElement.getBoundingClientRect();
        void getComputedStyle(document.body).backgroundColor;
        if (!(await commands.uiReady())) throw new Error("native startup gate rejected UI");
      } catch {
        // The native watchdog owns the fail-closed exit. Keep page/native
        // details out of this static diagnostic and never show partial chrome.
        console.error("trusted UI initialization failed");
      }
    })();
    onCleanup(() => theme.dispose());
  });

  if (getCurrentWindow().label === "panel") {
    return <Launcher />;
  }

  onMount(() => {
    void operations.init();
    void blocker.init();
    void tabs.init();
    void ui.init();
    if (!IS_MAC) void layout.init();
    document.addEventListener("keydown", onKeyDown);
    onCleanup(() => {
      operations.dispose();
      blocker.dispose();
      tabs.dispose();
      ui.dispose();
      if (!IS_MAC) layout.dispose();
      document.removeEventListener("keydown", onKeyDown);
    });
  });
  return (
    <>
      <Shell />
      <Show when={!IS_MAC}>
        <Dividers />
      </Show>
    </>
  );
}

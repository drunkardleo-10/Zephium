import { Show } from "solid-js";
import { NewTab } from "../features/newtab/NewTab";
import { Sidebar } from "../features/sidebar/Sidebar";
import * as tabs from "../state/tabs";

const IS_MAC = navigator.userAgent.includes("Mac");

// On macOS the chrome webview is the sidebar rect itself; on Windows/Linux
// it spans the whole window, so the shell's 8px window padding is DOM-side.
export function Shell() {
  return (
    <div class="flex h-screen w-screen" classList={{ "p-2": !IS_MAC }}>
      <Sidebar />
      <Show when={!tabs.activeTab()?.url}>
        <div class="min-w-0 flex-1">
          <NewTab />
        </div>
      </Show>
    </div>
  );
}

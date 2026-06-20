import { Show } from "solid-js";
import * as tabs from "../state/tabs";
import { Sidebar } from "../features/sidebar/Sidebar";
import { NewTab } from "../features/newtab/NewTab";

export function Shell() {
  return (
    <div class="flex h-screen w-screen">
      <Sidebar />
      <Show when={!tabs.activeTab()?.url}>
        <div class="min-w-0 flex-1">
          <NewTab />
        </div>
      </Show>
    </div>
  );
}

import { For } from "solid-js";
import * as tabs from "../../state/tabs";

export function Sidebar() {
  return (
    <aside class="flex w-[248px] shrink-0 select-none flex-col border-r border-border bg-sidebar">
      <div class="flex h-11 items-center px-4 text-[13px] font-semibold tracking-tight text-muted">
        Zephium
      </div>

      <nav class="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2.5 py-1">
        <For each={tabs.tabs()}>
          {(tab) => (
            <button
              onClick={() => tabs.activate(tab.id)}
              class="group flex h-9 items-center gap-2.5 rounded-md px-3 text-[13px]"
              classList={{
                "bg-elevated text-text": tab.id === tabs.activeId(),
                "text-muted hover:bg-hover hover:text-text": tab.id !== tabs.activeId(),
              }}
            >
              <span
                class="h-3.5 w-3.5 shrink-0 rounded-full bg-faint/40"
                classList={{ "animate-pulse bg-accent/70": tab.loading }}
              />
              <span class="flex-1 truncate text-left">{tab.title}</span>
              <span
                onClick={(e) => {
                  e.stopPropagation();
                  tabs.close(tab.id);
                }}
                class="hidden h-5 w-5 items-center justify-center rounded text-faint hover:bg-hover hover:text-text group-hover:flex"
              >
                ×
              </span>
            </button>
          )}
        </For>
      </nav>

      <div class="px-2.5 pb-3 pt-1">
        <button
          onClick={() => tabs.open()}
          class="flex h-9 w-full items-center gap-2 rounded-md px-3 text-[13px] text-muted hover:bg-hover hover:text-text"
        >
          <span class="text-base leading-none">+</span> New Tab
        </button>
      </div>
    </aside>
  );
}

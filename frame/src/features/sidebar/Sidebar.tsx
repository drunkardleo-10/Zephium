import { For, createSignal } from "solid-js";

interface Tab {
  id: number;
  title: string;
}

export function Sidebar() {
  const [tabs] = createSignal<Tab[]>([
    { id: 1, title: "New Tab" },
    { id: 2, title: "GitHub" },
    { id: 3, title: "Hacker News" },
  ]);
  const [active, setActive] = createSignal(1);

  return (
    <aside class="flex w-[248px] shrink-0 select-none flex-col border-r border-border bg-sidebar">
      <div class="flex h-11 items-center px-4 text-[13px] font-semibold tracking-tight text-muted">
        Zephium
      </div>

      <div class="px-2.5 pb-2">
        <button class="flex h-9 w-full items-center rounded-md bg-surface px-3 text-[13px] text-muted hover:text-text">
          Search or enter address
        </button>
      </div>

      <nav class="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2.5 py-1">
        <For each={tabs()}>
          {(tab) => (
            <button
              onClick={() => setActive(tab.id)}
              class="flex h-9 items-center gap-2.5 rounded-md px-3 text-[13px]"
              classList={{
                "bg-elevated text-text": active() === tab.id,
                "text-muted hover:bg-hover hover:text-text": active() !== tab.id,
              }}
            >
              <span class="h-3.5 w-3.5 shrink-0 rounded-full bg-faint/40" />
              <span class="truncate">{tab.title}</span>
            </button>
          )}
        </For>
      </nav>

      <div class="px-2.5 pb-3 pt-1">
        <button class="flex h-9 w-full items-center gap-2 rounded-md px-3 text-[13px] text-muted hover:bg-hover hover:text-text">
          <span class="text-base leading-none">+</span> New Tab
        </button>
      </div>
    </aside>
  );
}

import { For } from "solid-js";

const SHORTCUTS = ["GitHub", "YouTube", "Reddit", "X", "Wikipedia", "Hacker News", "MDN", "Gmail"];

export function NewTab() {
  return (
    <div class="flex h-full flex-col items-center justify-center gap-10 px-6">
      <div class="text-[26px] font-semibold tracking-tight text-text">Zephium</div>

      <div class="w-full max-w-[560px]">
        <input
          class="h-12 w-full rounded-xl border border-border bg-surface px-5 text-[15px] text-text outline-none placeholder:text-faint focus:border-accent"
          placeholder="Search the web or enter an address"
          spellcheck={false}
        />
      </div>

      <div class="grid w-full max-w-[560px] grid-cols-4 gap-3">
        <For each={SHORTCUTS}>
          {(name) => (
            <button class="flex flex-col items-center gap-2 rounded-xl px-2 py-4 hover:bg-hover">
              <span class="flex h-11 w-11 items-center justify-center rounded-full bg-elevated text-[15px] text-muted">
                {name[0]}
              </span>
              <span class="truncate text-[12px] text-muted">{name}</span>
            </button>
          )}
        </For>
      </div>
    </div>
  );
}

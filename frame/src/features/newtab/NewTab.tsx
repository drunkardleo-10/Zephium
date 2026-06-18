import { For } from "solid-js";
import * as tabs from "../../state/tabs";

const SHORTCUTS = ["GitHub", "YouTube", "Reddit", "Wikipedia", "Hacker News", "MDN", "X", "Gmail"];

export function NewTab() {
  let input!: HTMLInputElement;

  const go = (value: string) => {
    const id = tabs.activeId();
    if (id != null && value.trim()) tabs.navigate(id, value);
  };

  return (
    <div class="flex h-full flex-col items-center justify-center gap-10 px-6">
      <div class="text-[26px] font-semibold tracking-tight text-text">Zephium</div>

      <form
        class="w-full max-w-[560px]"
        onSubmit={(e) => {
          e.preventDefault();
          go(input.value);
        }}
      >
        <input
          ref={input}
          class="h-12 w-full rounded-xl border border-border bg-surface px-5 text-[15px] text-text outline-none placeholder:text-faint focus:border-accent"
          placeholder="Search the web or enter an address"
          spellcheck={false}
          autofocus
        />
      </form>

      <div class="grid w-full max-w-[560px] grid-cols-4 gap-3">
        <For each={SHORTCUTS}>
          {(name) => (
            <button
              onClick={() => go(`${name.toLowerCase().replace(/\s+/g, "")}.com`)}
              class="flex flex-col items-center gap-2 rounded-xl px-2 py-4 hover:bg-hover"
            >
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

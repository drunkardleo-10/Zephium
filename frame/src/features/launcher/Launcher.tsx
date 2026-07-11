import { getCurrentWindow } from "@tauri-apps/api/window";
import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { SearchResult } from "../../ipc/bindings";
import { commands, events } from "../../ipc/bindings";
import { FavIcon } from "../../ui/FavIcon";

const KIND_LABEL: Record<string, string> = {
  tab: "Tab",
  url: "Open",
  search: "Search",
  command: "Command",
  history: "History",
};

const KIND_GLYPH: Record<string, string> = {
  search: "?",
  command: ">",
  url: "@",
};

function accel(detail: string): string {
  if (!detail) return "";
  return detail
    .replace("CmdOrCtrl+", "⌘")
    .replace("Ctrl+", "⌃")
    .replace("Shift+", "⇧")
    .replace("Space", "␣");
}

export function Launcher() {
  let input!: HTMLInputElement;
  const [value, setValue] = createSignal("");
  const [results, setResults] = createSignal<SearchResult[]>([]);
  const [selected, setSelected] = createSignal(0);

  const search = (q: string) => {
    setValue(q);
    void commands.launcherSearch(q);
  };

  onMount(() => {
    input.focus();
    search("");
    const unlistenResults = events.searchChanged.listen((e) => {
      if (e.payload.query === value()) {
        setResults(e.payload.results);
        setSelected(0);
      }
    });
    const unlistenFocus = getCurrentWindow().onFocusChanged(({ payload }) => {
      if (payload) {
        input.focus();
        input.select();
        search(input.value);
      }
    });
    onCleanup(() => {
      void unlistenResults.then((f) => f());
      void unlistenFocus.then((f) => f());
    });
  });

  const run = (result: SearchResult | undefined) => {
    if (result) void commands.launcherRun(result.action);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const len = results().length;
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        void commands.panelHide();
        break;
      case "ArrowDown":
        e.preventDefault();
        if (len) setSelected((selected() + 1) % len);
        break;
      case "ArrowUp":
        e.preventDefault();
        if (len) setSelected((selected() + len - 1) % len);
        break;
      case "Enter":
        e.preventDefault();
        run(results()[selected()]);
        break;
    }
  };

  return (
    <div class="shell flex h-screen w-screen flex-col">
      <input
        ref={input}
        value={value()}
        onKeyDown={onKeyDown}
        onInput={(e) => search(e.currentTarget.value)}
        placeholder="Search or enter address"
        spellcheck={false}
        class="h-14 w-full shrink-0 border-b border-border bg-transparent px-5 text-[15px] text-text outline-none placeholder:text-faint"
      />
      <div class="flex-1 overflow-y-auto p-2">
        <For each={results()}>
          {(result, index) => (
            <button
              type="button"
              onMouseMove={() => setSelected(index())}
              onClick={() => run(result)}
              class="flex h-11 w-full items-center gap-3 rounded-lg px-3 text-left"
              classList={{ "bg-hover": index() === selected() }}
            >
              <span class="flex h-5 w-5 shrink-0 items-center justify-center">
                <Show
                  when={result.favicon}
                  fallback={
                    <span class="text-[12px] text-faint">{KIND_GLYPH[result.kind] ?? ""}</span>
                  }
                >
                  <FavIcon favicon={result.favicon} />
                </Show>
              </span>
              <span class="w-14 shrink-0 text-[10px] uppercase tracking-wide text-faint">
                {KIND_LABEL[result.kind] ?? result.kind}
              </span>
              <span class="min-w-0 flex-1 truncate text-[13.5px] text-text">{result.title}</span>
              <span class="max-w-56 shrink-0 truncate text-[12px] text-faint">
                {result.kind === "command" ? accel(result.detail) : result.detail}
              </span>
            </button>
          )}
        </For>
        <Show when={results().length === 0 && value().trim() !== ""}>
          <div class="px-3 py-2 text-[12px] text-faint">No results</div>
        </Show>
      </div>
    </div>
  );
}

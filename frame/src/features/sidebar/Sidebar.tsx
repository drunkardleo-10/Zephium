import { For, Show, createEffect, createSignal, type JSX } from "solid-js";
import * as tabs from "../../state/tabs";

function host(url: string | null | undefined): string {
  if (!url) return "";
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

export function Sidebar() {
  let input!: HTMLInputElement;
  const [value, setValue] = createSignal("");
  const [editing, setEditing] = createSignal(false);
  const [splitting, setSplitting] = createSignal(false);

  const onTab = (id: number) => {
    if (splitting()) {
      tabs.split(id);
      setSplitting(false);
    } else {
      tabs.activate(id);
    }
  };

  createEffect(() => {
    const h = host(tabs.activeTab()?.url);
    if (!editing()) setValue(h);
  });

  const submit = (e: SubmitEvent) => {
    e.preventDefault();
    const id = tabs.activeId();
    if (id != null) tabs.navigate(id, value());
    input.blur();
  };

  return (
    <aside class="flex w-60 shrink-0 select-none flex-col">
      <div data-tauri-drag-region class="h-8 shrink-0" />

      <div class="flex items-center gap-0.5 px-2.5">
        <NavButton label="Back" onClick={tabs.backActive}>‹</NavButton>
        <NavButton label="Forward" onClick={tabs.forwardActive}>›</NavButton>
        <NavButton label="Reload" onClick={tabs.reloadActive}>⟳</NavButton>
        <NavButton
          label="Split"
          active={splitting()}
          onClick={() => setSplitting((s) => !s)}
        >
          ⊟
        </NavButton>
      </div>

      <form class="px-2.5 pb-2 pt-1" onSubmit={submit}>
        <input
          ref={input}
          value={value()}
          onInput={(e) => setValue(e.currentTarget.value)}
          onFocus={(e) => {
            setEditing(true);
            e.currentTarget.select();
          }}
          onBlur={() => setEditing(false)}
          placeholder="Search or enter site"
          spellcheck={false}
          class="h-8 w-full rounded-lg bg-elevated/60 px-3 text-[12.5px] text-text outline-none placeholder:text-faint focus:bg-elevated"
        />
      </form>

      <Show when={splitting()}>
        <div class="px-3 pb-1 text-[11px] text-faint">Pick a tab to split with</div>
      </Show>

      <nav class="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2.5 py-1">
        <For each={tabs.tabs()}>
          {(tab) => (
            <button
              onClick={() => onTab(tab.id)}
              class="group flex h-9 items-center gap-2.5 rounded-md px-3 text-[13px]"
              classList={{
                "ring-1 ring-accent/40": splitting() && tab.id !== tabs.activeId(),
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

function NavButton(props: {
  label: string;
  onClick: () => void;
  active?: boolean;
  children: JSX.Element;
}) {
  return (
    <button
      aria-label={props.label}
      onClick={props.onClick}
      class="flex h-7 w-7 items-center justify-center rounded-md"
      classList={{
        "bg-accent/20 text-text": props.active,
        "text-muted hover:bg-hover hover:text-text": !props.active,
      }}
    >
      {props.children}
    </button>
  );
}

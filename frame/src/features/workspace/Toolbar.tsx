import { createEffect, createSignal, type JSX } from "solid-js";
import * as tabs from "../../state/tabs";

export function Toolbar() {
  let input!: HTMLInputElement;
  const [value, setValue] = createSignal("");
  const [editing, setEditing] = createSignal(false);

  createEffect(() => {
    const url = tabs.activeTab()?.url ?? "";
    if (!editing()) setValue(url);
  });

  const submit = (e: SubmitEvent) => {
    e.preventDefault();
    const id = tabs.activeId();
    if (id != null) tabs.navigate(id, value());
    input.blur();
  };

  return (
    <header class="flex h-11 shrink-0 select-none items-center gap-1 border-b border-border px-3">
      <NavButton label="Back" onClick={tabs.backActive}>‹</NavButton>
      <NavButton label="Forward" onClick={tabs.forwardActive}>›</NavButton>
      <NavButton label="Reload" onClick={tabs.reloadActive}>⟳</NavButton>
      <form class="ml-1 flex-1" onSubmit={submit}>
        <input
          ref={input}
          value={value()}
          onInput={(e) => setValue(e.currentTarget.value)}
          onFocus={(e) => {
            setEditing(true);
            e.currentTarget.select();
          }}
          onBlur={() => setEditing(false)}
          placeholder="Search or enter address"
          spellcheck={false}
          class="h-8 w-full rounded-lg border border-border bg-surface px-3.5 text-[13px] text-text outline-none placeholder:text-faint focus:border-accent"
        />
      </form>
    </header>
  );
}

function NavButton(props: { label: string; onClick: () => void; children: JSX.Element }) {
  return (
    <button
      aria-label={props.label}
      onClick={props.onClick}
      class="flex h-7 w-7 items-center justify-center rounded-md text-muted hover:bg-hover hover:text-text"
    >
      {props.children}
    </button>
  );
}

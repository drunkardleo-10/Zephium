import { createEffect, createSignal } from "solid-js";
import type { Tab } from "../state/tabs";

interface Props {
  tab: Tab | undefined;
  onSubmit: (value: string) => void;
  onBack: () => void;
  onForward: () => void;
  onReload: () => void;
}

export function Toolbar(props: Props) {
  let input!: HTMLInputElement;
  const [value, setValue] = createSignal("");
  const [editing, setEditing] = createSignal(false);

  // Reflect the active tab's URL unless the user is editing the field.
  createEffect(() => {
    const url = props.tab?.url ?? "";
    if (!editing()) setValue(url);
  });

  const submit = (e: Event) => {
    e.preventDefault();
    props.onSubmit(value());
    input.blur();
  };

  return (
    <div class="toolbar">
      <div class="nav-buttons">
        <button class="nav-btn" aria-label="Back" onClick={props.onBack}>
          <svg viewBox="0 0 16 16" width="16" height="16">
            <path d="M10 3 L5 8 L10 13" fill="none" stroke="currentColor" stroke-width="1.6" />
          </svg>
        </button>
        <button class="nav-btn" aria-label="Forward" onClick={props.onForward}>
          <svg viewBox="0 0 16 16" width="16" height="16">
            <path d="M6 3 L11 8 L6 13" fill="none" stroke="currentColor" stroke-width="1.6" />
          </svg>
        </button>
        <button class="nav-btn" aria-label="Reload" onClick={props.onReload}>
          <svg viewBox="0 0 16 16" width="16" height="16">
            <path
              d="M13 8 a5 5 0 1 1 -1.5 -3.6 M13 2.5 L13 5 L10.5 5"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
            />
          </svg>
        </button>
      </div>

      <form class="address-form" onSubmit={submit}>
        <div class="loadbar" classList={{ active: props.tab?.loading ?? false }} />
        <input
          ref={input}
          class="address"
          value={value()}
          placeholder="Search or enter address"
          spellcheck={false}
          autocomplete="off"
          autocapitalize="off"
          autocorrect="off"
          onInput={(e) => setValue(e.currentTarget.value)}
          onFocus={(e) => {
            setEditing(true);
            e.currentTarget.select();
          }}
          onBlur={() => setEditing(false)}
        />
      </form>
    </div>
  );
}

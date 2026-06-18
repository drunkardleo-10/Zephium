import type { JSX } from "solid-js";

export function Toolbar() {
  return (
    <header class="flex h-11 shrink-0 select-none items-center gap-1 border-b border-border px-3">
      <NavButton label="Back">‹</NavButton>
      <NavButton label="Forward">›</NavButton>
      <NavButton label="Reload">⟳</NavButton>
      <div class="ml-2 flex-1 truncate text-[12.5px] text-faint">New Tab</div>
    </header>
  );
}

function NavButton(props: { label: string; children: JSX.Element }) {
  return (
    <button
      aria-label={props.label}
      class="flex h-7 w-7 items-center justify-center rounded-md text-muted hover:bg-hover hover:text-text"
    >
      {props.children}
    </button>
  );
}

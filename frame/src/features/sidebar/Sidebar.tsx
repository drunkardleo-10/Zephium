import { For, Show, createEffect, createSignal, type JSX } from "solid-js";
import * as tabs from "../../state/tabs";
import * as ui from "../../state/ui";

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
  const [width, setWidth] = createSignal(240);

  const onTab = (id: string) => {
    if (splitting()) {
      tabs.split(id);
      setSplitting(false);
    } else {
      tabs.activate(id);
    }
  };

  let resizing = false;
  let frame = 0;
  const onResizeDown = (e: PointerEvent & { currentTarget: HTMLElement }) => {
    resizing = true;
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onResizeMove = (e: PointerEvent) => {
    if (!resizing) return;
    const w = Math.max(180, Math.min(420, e.clientX));
    setWidth(w);
    if (!frame) {
      frame = requestAnimationFrame(() => {
        frame = 0;
        tabs.setSidebarWidth(width());
      });
    }
  };
  const onResizeUp = (e: PointerEvent & { currentTarget: HTMLElement }) => {
    resizing = false;
    if (frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
    tabs.setSidebarWidth(width());
    e.currentTarget.releasePointerCapture(e.pointerId);
  };

  const [ghost, setGhost] = createSignal<{ title: string; x: number; y: number } | null>(null);
  let down: { x: number; y: number } | null = null;
  let dragId = "";
  let dragTitle = "";
  let dragging = false;
  let dragFrame = 0;
  let suppressClick = false;
  const onTabDown = (e: PointerEvent, id: string, title: string) => {
    down = { x: e.clientX, y: e.clientY };
    dragId = id;
    dragTitle = title;
    dragging = false;
  };
  const onTabMove = (e: PointerEvent & { currentTarget: HTMLElement }) => {
    if (!down) return;
    if (!dragging && Math.abs(e.clientX - down.x) + Math.abs(e.clientY - down.y) > 4) {
      dragging = true;
      e.currentTarget.setPointerCapture(e.pointerId);
      document.body.style.cursor = "grabbing";
    }
    if (dragging) {
      setGhost({ title: dragTitle, x: e.clientX, y: e.clientY });
      if (!dragFrame) {
        const x = e.clientX;
        const y = e.clientY;
        dragFrame = requestAnimationFrame(() => {
          dragFrame = 0;
          tabs.dragOver(x, y);
        });
      }
    }
  };
  const onTabUp = (e: PointerEvent & { currentTarget: HTMLElement }) => {
    if (dragFrame) {
      cancelAnimationFrame(dragFrame);
      dragFrame = 0;
    }
    if (dragging) {
      e.currentTarget.releasePointerCapture(e.pointerId);
      tabs.dropTab(dragId, e.clientX, e.clientY);
      suppressClick = true;
    }
    setGhost(null);
    document.body.style.cursor = "";
    down = null;
    dragging = false;
  };
  const onTabClick = (id: string) => {
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    onTab(id);
  };

  createEffect(() => {
    const h = host(tabs.activeTab()?.url);
    if (!editing()) setValue(h);
  });

  createEffect(() => {
    const cmd = ui.uiCommand();
    if (cmd.seq > 0 && cmd.id === "url.focus") {
      input.focus();
      input.select();
    }
  });

  const submit = (e: SubmitEvent) => {
    e.preventDefault();
    const id = tabs.activeId();
    if (id != null) tabs.navigate(id, value());
    input.blur();
  };

  return (
    <aside
      style={{ width: `${width()}px` }}
      class="relative flex shrink-0 select-none flex-col"
    >
      <div
        onPointerDown={onResizeDown}
        onPointerMove={onResizeMove}
        onPointerUp={onResizeUp}
        class="absolute right-0 top-0 z-10 h-full w-1.5 cursor-col-resize hover:bg-accent/20"
      />
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
              onPointerDown={(e) => onTabDown(e, tab.id, tab.title)}
              onPointerMove={onTabMove}
              onPointerUp={onTabUp}
              onClick={() => onTabClick(tab.id)}
              class="group flex h-9 cursor-grab items-center gap-2.5 rounded-md px-3 text-[13px]"
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

      <Show when={ghost()}>
        {(g) => (
          <div
            class="pointer-events-none fixed z-50 max-w-44 truncate rounded-md bg-elevated px-3 py-1.5 text-[13px] text-text shadow-lg"
            style={{ left: `${g().x + 12}px`, top: `${g().y + 6}px` }}
          >
            {g().title}
          </div>
        )}
      </Show>
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

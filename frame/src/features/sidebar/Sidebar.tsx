import {
  Add01Icon,
  ArrowLeft01Icon,
  ArrowRight01Icon,
  Cancel01Icon,
  Maximize01Icon,
  MinusSignIcon,
  MoreHorizontalIcon,
  RefreshIcon,
  TableColumnsSplitIcon,
} from "@hugeicons/core-free-icons";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createEffect, createSignal, For, type JSX, Show } from "solid-js";
import { commands } from "../../ipc/bindings";
import * as tabs from "../../state/tabs";
import * as ui from "../../state/ui";
import { FavIcon } from "../../ui/FavIcon";
import { Icon } from "../../ui/Icon";
import { BlockerStatus } from "./BlockerStatus";

const IS_MAC = navigator.userAgent.includes("Mac");

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
      data-zephium-active-tab={tabs.activeId() ?? ""}
      data-tauri-drag-region
      style={{ width: `${width()}px` }}
      class="relative flex shrink-0 select-none flex-col"
    >
      <div
        onPointerDown={onResizeDown}
        onPointerMove={onResizeMove}
        onPointerUp={onResizeUp}
        class="absolute right-0 top-0 z-10 h-full w-1.5 cursor-col-resize hover:bg-accent/20"
      />
      <div data-tauri-drag-region class="flex h-8 shrink-0 items-center justify-end">
        <Show when={!IS_MAC}>
          <div class="flex gap-0.5 pr-1.5">
            <WindowButton label="Minimize" onClick={() => void getCurrentWindow().minimize()}>
              <Icon icon={MinusSignIcon} size={13} />
            </WindowButton>
            <WindowButton label="Maximize" onClick={() => void getCurrentWindow().toggleMaximize()}>
              <Icon icon={Maximize01Icon} size={11} />
            </WindowButton>
            <WindowButton label="Close" onClick={() => void getCurrentWindow().close()}>
              <Icon icon={Cancel01Icon} size={13} />
            </WindowButton>
          </div>
        </Show>
      </div>

      <div class="flex items-center gap-0.5 px-2.5">
        <NavButton label="Back" onClick={tabs.backActive}>
          <Icon icon={ArrowLeft01Icon} />
        </NavButton>
        <NavButton label="Forward" onClick={tabs.forwardActive}>
          <Icon icon={ArrowRight01Icon} />
        </NavButton>
        <NavButton label="Reload" onClick={tabs.reloadActive}>
          <Icon icon={RefreshIcon} size={15} />
        </NavButton>
        <NavButton label="Split" active={splitting()} onClick={() => setSplitting((s) => !s)}>
          <Icon icon={TableColumnsSplitIcon} size={15} />
        </NavButton>
        <Show when={!IS_MAC}>
          <NavButton
            label="Menu"
            onClick={(event) => {
              const anchor = event.currentTarget.getBoundingClientRect();
              void commands.menuPopup(anchor.left, anchor.bottom);
            }}
          >
            <Icon icon={MoreHorizontalIcon} />
          </NavButton>
        </Show>
      </div>

      <form class="px-2.5 pb-2 pt-1" onSubmit={submit}>
        <input
          data-zephium-address
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
            <div
              data-zephium-tab-id={tab.id}
              data-zephium-tab-url={tab.url ?? ""}
              data-zephium-projection-revision={tab.projection_revision}
              class="group flex h-9 items-center rounded-md text-[13px]"
              classList={{
                "ring-1 ring-accent/40": splitting() && tab.id !== tabs.activeId(),
                "bg-elevated text-text": tab.id === tabs.activeId(),
                "text-muted hover:bg-hover hover:text-text": tab.id !== tabs.activeId(),
              }}
            >
              <button
                type="button"
                onPointerDown={(e) => onTabDown(e, tab.id, tab.title)}
                onPointerMove={onTabMove}
                onPointerUp={onTabUp}
                onClick={() => onTabClick(tab.id)}
                class="flex min-w-0 flex-1 cursor-grab items-center gap-2.5 self-stretch px-3"
              >
                <FavIcon favicon={tab.favicon} loading={tab.loading} />
                <span data-zephium-tab-label class="flex-1 truncate text-left">
                  {tab.title}
                </span>
              </button>
              <button
                type="button"
                aria-label="Close tab"
                onClick={(e) => {
                  e.stopPropagation();
                  tabs.close(tab.id);
                }}
                class="mr-2 flex h-5 w-5 items-center justify-center rounded text-faint opacity-0 hover:bg-hover hover:text-text group-hover:opacity-100 focus:opacity-100"
              >
                <Icon icon={Cancel01Icon} size={12} />
              </button>
            </div>
          )}
        </For>
      </nav>

      <BlockerStatus />

      <div class="px-2.5 pb-3 pt-1">
        <button
          type="button"
          onClick={() => tabs.open()}
          class="flex h-9 w-full items-center gap-2 rounded-md px-3 text-[13px] text-muted hover:bg-hover hover:text-text"
        >
          <Icon icon={Add01Icon} size={15} /> New Tab
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

function WindowButton(props: { label: string; onClick: () => void; children: JSX.Element }) {
  return (
    <button
      type="button"
      aria-label={props.label}
      title={props.label}
      onClick={props.onClick}
      class="flex h-6 w-7 items-center justify-center rounded-md text-faint hover:bg-hover hover:text-text"
    >
      {props.children}
    </button>
  );
}

function NavButton(props: {
  label: string;
  onClick: (event: MouseEvent & { currentTarget: HTMLButtonElement }) => void;
  active?: boolean;
  children: JSX.Element;
}) {
  return (
    <button
      type="button"
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

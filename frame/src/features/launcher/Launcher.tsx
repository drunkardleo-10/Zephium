import { createSignal, onCleanup, onMount } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { commands } from "../../ipc/bindings";

export function Launcher() {
  let input!: HTMLInputElement;
  const [value, setValue] = createSignal("");

  onMount(() => {
    input.focus();
    const unlisten = getCurrentWindow().onFocusChanged(({ payload }) => {
      if (payload) {
        input.focus();
        input.select();
      }
    });
    onCleanup(() => void unlisten.then((f) => f()));
  });

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      void commands.panelHide();
    }
  };

  return (
    <div class="flex h-screen w-screen flex-col" onKeyDown={onKeyDown}>
      <input
        ref={input}
        value={value()}
        onInput={(e) => setValue(e.currentTarget.value)}
        placeholder="Search or enter address"
        spellcheck={false}
        class="h-14 w-full shrink-0 border-b border-white/10 bg-transparent px-5 text-[15px] text-text outline-none placeholder:text-faint"
      />
      <div class="flex-1 overflow-y-auto px-2 py-2">
        <div class="px-3 py-2 text-[12px] text-faint">
          Tabs, history and commands land here next.
        </div>
      </div>
    </div>
  );
}

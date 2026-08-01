<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import Launcher from "../features/launcher/Launcher.svelte";
  import * as sidebar from "../features/sidebar/sidebar-mode.svelte";
  import Dividers from "../features/split/Dividers.svelte";
  import { commands } from "../shared/ipc/bindings";
  import { IS_MAC } from "../shared/platform";
  import * as blocker from "../domain/blocker/blocker.svelte";
  import * as layout from "../features/split/layout.svelte";
  import * as operations from "../domain/operations/operations";
  import * as runtime from "../domain/runtime/runtime.svelte";
  import * as tabs from "../domain/tabs/tabs.svelte";
  import * as theme from "../domain/theme/theme";
  import * as ui from "../domain/ui-commands/ui-commands.svelte";
  import Shell from "./Shell.svelte";

  const currentWindow = getCurrentWindow();
  const isPanel = currentWindow.label === "panel";

  type ChromeShortcut = {
    matches: (event: KeyboardEvent) => boolean;
    command: string;
  };

  const chromeShortcuts: ChromeShortcut[] = [
    {
      matches: (event) => event.ctrlKey && !event.shiftKey && event.key === "Tab",
      command: "tab.next",
    },
    {
      matches: (event) => event.ctrlKey && event.shiftKey && event.key === "Tab",
      command: "tab.previous",
    },
  ];

  if (!IS_MAC) {
    const primaryShortcuts: ReadonlyArray<readonly [string, string]> = [
      ["t", "tab.new"],
      ["w", "tab.close"],
      ["r", "nav.reload"],
      ["l", "url.focus"],
      ["=", "zoom.in"],
      ["-", "zoom.out"],
      ["0", "zoom.reset"],
      ["[", "nav.back"],
      ["]", "nav.forward"],
      [".", "nav.stop"],
    ];

    for (const [key, command] of primaryShortcuts) {
      chromeShortcuts.push({
        matches: (event) =>
          event.ctrlKey && !event.shiftKey && !event.altKey && event.key.toLowerCase() === key,
        command,
      });
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    for (const shortcut of chromeShortcuts) {
      if (!shortcut.matches(event)) continue;
      event.preventDefault();
      void commands.runCommand(shortcut.command);
      return;
    }
  }

  onMount(() => {
    let disposed = false;

    // Each async initializer performs its synchronous setup before its first
    // await. In particular, tabs installs every scoped projection listener
    // before bootstrap can ask native code for a snapshot.
    const themeReady = theme.init();

    if (isPanel) {
      void themeReady.catch(() => {
        console.error("trusted UI initialization failed");
      });
      return () => {
        disposed = true;
        theme.dispose();
      };
    }

    // Runtime status is emitted by the same actor-ordered bootstrap that
    // supplies tabs, so its listener must exist before tabs starts bootstrap.
    const runtimeReady = runtime.init();
    const tabsReady = tabs.init();
    const sidebarReady = sidebar.init();
    const uiEventsReady = ui.init();
    void operations.init();
    void blocker.init();
    if (!IS_MAC) void layout.init();
    document.addEventListener("keydown", handleKeydown);

    void (async () => {
      try {
        await Promise.all([themeReady, runtimeReady, tabsReady, uiEventsReady, sidebarReady]);
        if (disposed) return;

        // A hidden native window can suspend animation frames indefinitely.
        // Resolve style/layout synchronously before admitting native reveal.
        void document.documentElement.getBoundingClientRect();
        void getComputedStyle(document.body).backgroundColor;
        if (!(await commands.uiReady())) {
          throw new Error("native startup gate rejected UI");
        }
      } catch {
        // Native owns the fail-closed watchdog. Keep attacker-controlled page
        // data and native internals out of this static diagnostic.
        console.error("trusted UI initialization failed");
      }
    })();

    return () => {
      disposed = true;
      theme.dispose();
      operations.dispose();
      blocker.dispose();
      runtime.dispose();
      tabs.dispose();
      ui.dispose();
      if (!IS_MAC) layout.dispose();
      document.removeEventListener("keydown", handleKeydown);
    };
  });
</script>

{#if isPanel}
  <Launcher />
{:else}
  <Shell />
  {#if !IS_MAC}
    <Dividers />
  {/if}
{/if}

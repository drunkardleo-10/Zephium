<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import Launcher from "../features/launcher/Launcher.svelte";
  import Dividers from "../features/split/Dividers.svelte";
  import { commands } from "../ipc/bindings";
  import * as blocker from "../state/blocker.svelte";
  import * as layout from "../state/layout.svelte";
  import * as operations from "../state/operations";
  import * as tabs from "../state/tabs.svelte";
  import * as theme from "../state/theme";
  import * as ui from "../state/ui.svelte";
  import Shell from "./Shell.svelte";

  const currentWindow = getCurrentWindow();
  const isPanel = currentWindow.label === "panel";
  const isMac = navigator.userAgent.includes("Mac");

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

  if (!isMac) {
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

    const tabsReady = tabs.init();
    const uiEventsReady = ui.init();
    void operations.init();
    void blocker.init();
    if (!isMac) void layout.init();
    document.addEventListener("keydown", handleKeydown);

    void (async () => {
      try {
        await Promise.all([themeReady, tabsReady, uiEventsReady]);
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
      tabs.dispose();
      ui.dispose();
      if (!isMac) layout.dispose();
      document.removeEventListener("keydown", handleKeydown);
    };
  });
</script>

{#if isPanel}
  <Launcher />
{:else}
  <Shell />
  {#if !isMac}
    <Dividers />
  {/if}
{/if}

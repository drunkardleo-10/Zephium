<script lang="ts">
  import { installCloseService } from "$shared/lib/close";
  import "$styles/global.css";
  import { onMount, untrack } from "svelte";
  import { WebExtensionAccessPrompt, WebExtensionReview } from "$features/webext";
  import { webext } from "$domain/webext";
  import { PagePermissionPrompt } from "$features/permissions";
  import * as sidebar from "$session/sidebar-mode.svelte";
  import { Dividers } from "$features/split";
  import { commands } from "$shared/ipc/bindings";
  import { IS_MAC } from "$shared/platform";
  import { blocker } from "$domain/blocker";
  import { extensions } from "$domain/extensions";
  import { favicons } from "$domain/favicons";
  import { layout } from "$domain/layout";
  import { operations } from "$domain/operations";
  import { pagePermissions } from "$domain/permissions";
  import { runtime } from "$domain/runtime";
  import { tabs } from "$domain/tabs";
  import { theme } from "$domain/appearance";
  import { uiCommands as ui } from "$domain/ui-commands";
  import { surface as browserPage } from "$domain/surface";
  import * as motion from "$session/motion.svelte";
  import * as tools from "$session/tools.svelte";
  import { preferences } from "$domain/preferences";
  import Shell from "./Shell.svelte";

  // Follows the stored preference and nothing else. Adopting reads the
  // sidebar's own shape; tracked, that read made every toggle re-adopt the
  // not-yet-saved old value, bouncing the column back and forth and cutting
  // the page's slide short.
  $effect(() => {
    const mode = preferences.value("sidebar.mode");
    if (mode === "default" || mode === "compact") untrack(() => sidebar.adoptMode(mode));
  });

  $effect(() => {
    theme.applyUiPreferences(
      preferences.value("ui.accent"),
      preferences.value("ui.reduce-motion") === "true",
    );
  });

  let pagePermissionPrompt = $derived(pagePermissions.prompt());
  let consentActive = $derived(
    pagePermissionPrompt !== null || webext.review() !== null || webext.accessRequest() !== null,
  );

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
    // The native content stage is suppressed while this browser-owned modal
    // is active. Keep chrome shortcuts from mutating tabs behind it as well.
    if (consentActive) return;
    if ((event.metaKey || event.ctrlKey) && event.key === ",") {
      event.preventDefault();
      void browserPage.open("settings");
      return;
    }
    for (const shortcut of chromeShortcuts) {
      if (!shortcut.matches(event)) continue;
      event.preventDefault();
      void commands.runCommand(shortcut.command);
      return;
    }
  }

  onMount(installCloseService);
  onMount(() => {
    let disposed = false;

    // Each async initializer performs its synchronous setup before its first
    // await. In particular, tabs installs every scoped projection listener
    // before bootstrap can ask native code for a snapshot.
    const themeReady = theme.init();

    // Runtime status is emitted by the same actor-ordered bootstrap that
    // supplies tabs, so its listener must exist before tabs starts bootstrap.
    const toolsReady = tools.init();
    const browserPageReady = browserPage.init();
    const preferencesReady = preferences.init();
    const runtimeReady = runtime.init();
    const extensionsReady = extensions.init();
    void webext.refresh();
    const stopAccess = webext.listenForAccess();
    const stopDrops = webext.listenForDrops(() => browserPage.currentPage() !== "work");
    const pagePermissionsReady = pagePermissions.init();

    // Rasters are emitted immediately before the projection that references
    // them, so this listener must exist before tabs asks native to bootstrap.
    const faviconsReady = favicons.init();
    const tabsReady = tabs.init();
    const sidebarReady = sidebar.init();
    const uiEventsReady = ui.init();
    void operations.init();
    void blocker.init();
    if (!IS_MAC) void layout.init();
    document.addEventListener("keydown", handleKeydown);

    void (async () => {
      try {
        await Promise.all([
          toolsReady,
          browserPageReady,
          preferencesReady,
          themeReady,
          runtimeReady,
          extensionsReady,
          pagePermissionsReady,
          faviconsReady,
          tabsReady,
          uiEventsReady,
          sidebarReady,
        ]);
        if (disposed) return;

        // A hidden native window can suspend animation frames indefinitely.
        // Resolve style/layout synchronously before admitting native reveal.
        void document.documentElement.getBoundingClientRect();
        void getComputedStyle(document.body).backgroundColor;
        if (!(await commands.uiReady())) {
          throw new Error("native startup gate rejected UI");
        }
        motion.reveal();
      } catch {
        // Native owns the fail-closed watchdog. Keep attacker-controlled page
        // data and native internals out of this static diagnostic.
        console.error("trusted UI initialization failed");
      }
    })();

    return () => {
      disposed = true;
      theme.dispose();
      browserPage.dispose();
      tools.dispose();
      motion.dispose();
      preferences.dispose();
      operations.dispose();
      blocker.dispose();
      runtime.dispose();
      extensions.dispose();
      pagePermissions.dispose();
      void stopAccess.then((stop) => stop());
      void stopDrops.then((stop) => stop());
      favicons.dispose();
      tabs.dispose();
      ui.dispose();
      if (!IS_MAC) layout.dispose();
      document.removeEventListener("keydown", handleKeydown);
    };
  });
</script>

<div class="contents" inert={consentActive}>
  <Shell />
  {#if !IS_MAC}
    <Dividers />
  {/if}
</div>
{#if webext.review() !== null}
  <WebExtensionReview review={webext.review()!} />
{:else if webext.accessRequest() !== null}
  {#key webext.accessRequest()!.request}
    <WebExtensionAccessPrompt request={webext.accessRequest()!} />
  {/key}
{:else if pagePermissionPrompt !== null}
  {#key `${pagePermissionPrompt.profile_id}:${pagePermissionPrompt.item_id}:${pagePermissionPrompt.request_id}`}
    <PagePermissionPrompt prompt={pagePermissionPrompt} />
  {/key}
{/if}

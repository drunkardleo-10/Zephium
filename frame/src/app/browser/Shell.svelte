<script lang="ts">
  import { theme } from "$domain/appearance";
  import LazyView from "$shared/ui/LazyView";
  import RenderBoundary from "$shared/ui/RenderBoundary";
  import { loadSettings } from "$features/settings";
  import { EssentialTile } from "$features/essentials";
  import { AddressField } from "$features/address";
  import { EssentialsRail } from "$features/essentials";
  import { ExtensionActions } from "$features/extensions";
  import { SpaceHeader } from "$features/spaces";
  import { sidebarTree } from "$features/tabs";
  import { SidebarBody } from "$features/tabs";
  import { TabList } from "$features/tabs";
  import { TabRail } from "$features/tabs";
  import * as tabDrag from "$session/tab-drag.svelte";
  import { uiCommands as ui } from "$domain/ui-commands";
  import { untrack } from "svelte";
  import { BlockerShield } from "$features/blocker";

  import { preview } from "$features/settings";
  import { onMount } from "svelte";
  import { events } from "$shared/ipc/native-events";
  import { handleNativeSection } from "$features/settings";
  import * as m from "$shared/i18n/messages";
  import { surface as browserPage } from "$domain/surface";
  import { loadToolSlot } from "$features/tools";
  import * as toolHost from "$session/tools.svelte";
  import { SettingsNavigation } from "$features/settings";
  import { LibraryPage } from "$features/library";
  import { NewTab } from "$features/newtab";
  import { Sidebar } from "$features/sidebar";
  import { IS_MAC } from "$shared/platform";
  import { tabs } from "$domain/tabs";
  onMount(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void events.uiCommand
      .listen((event) => handleNativeSection(event.payload))
      .then((unsubscribe) => {
        if (disposed) unsubscribe();
        else stop = unsubscribe;
      });
    return () => {
      disposed = true;
      stop?.();
    };
  });
  $effect(() => {
    const text = preview.get("accessibility.text", "Default");
    theme.applyPreview({
      density:
        preview.get("appearance.density", "Comfortable") === "Compact" ? "compact" : "comfortable",
      contrast: preview.get("accessibility.contrast", false),
      text: text === "Large" ? "large" : text === "Small" ? "small" : "default",
    });
  });
  let splitting = $state(false);
  let tree = $derived(sidebarTree(tabs.sidebarNodes(), tabs.tabs()));
  // The rail shows one flat list. Folders and split grouping are shapes that
  // need labels, so they stay in the expanded body.
  let railTabs = $derived(
    tabs.tabs().filter((tab) => !railEssentials.some((essential) => essential.id === tab.id)),
  );
  let railEssentials = $derived(
    tree.favorites.flatMap((entry) => (entry.kind === "tab" ? [entry.tab] : [])),
  );

  let handledCommand = 0;
  $effect(() => {
    const command = ui.uiCommand();
    if (command.seq === 0 || command.seq === handledCommand) return;
    handledCommand = command.seq;
    untrack(() => {
      if (command.id === "split.choose") splitting = true;
      if (command.id === "tab.copyLink") tabs.copyMenuTargetLink();
    });
  });
  function selectTab(id: string) {
    if (splitting) {
      tabs.split(id);
      splitting = false;
      return;
    }
    tabs.activate(id);
  }
</script>

<div
  class="shell flex h-screen w-screen"
  class:p-2={!IS_MAC}
  data-zephium-active-tab={tabs.activeId() ?? ""}
  data-zephium-surface={browserPage.currentPage() === "settings" ? "settings" : "browse"}
>
  <Sidebar
    >{#snippet browserBody(compact)}
      <AddressField {compact}>{#snippet shield()}<BlockerShield />{/snippet}</AddressField>
      {#if compact}
        <ExtensionActions compact />
        <EssentialsRail entries={railEssentials} onSelect={selectTab} />
        <TabRail entries={railTabs} onSelect={selectTab} />
      {:else}
        <ExtensionActions />
        {#if tree.favorites.length > 0 || tabDrag.draggedId() !== null}<div
            class="essentials-drop-zone shrink-0 pb-1"
            data-essentials-drop
            data-over={tabDrag.overEssentials()}
          >
            {#if tree.favorites.length === 0}<div class="essential-drop-hint">
                {m.essential_drop_hint()}
              </div>{/if}
            <TabList
              entries={tree.favorites}
              section="favorites"
              variant="essentials"
              label={m.essentials()}
              {splitting}
              onSelect={selectTab}
              >{#snippet essentialTile(props)}<EssentialTile {...props} />{/snippet}</TabList
            >
          </div>{/if}
        {#if tabDrag.moveFailed()}<p class="sidebar-move-error" role="alert">
            {m.essential_move_failed()}
          </p>{/if}
        <SpaceHeader />
        {#if splitting}<p class="shrink-0 px-3 pb-1 text-[12px] text-accent" aria-live="polite">
            {m.choose_split()}
          </p>{/if}
        <SidebarBody pinned={tree.pinned} today={tree.today} {splitting} onSelect={selectTab} />
      {/if}
    {/snippet}{#snippet settingsNavigation()}<SettingsNavigation />{/snippet}{#snippet toolPanel(
      kind,
    )}<LazyView
        loader={loadToolSlot}
        loadingLabel={m.surface_loading()}
        failureLabel={m.surface_render_failed()}
        retryLabel={m.surface_retry()}
        >{#snippet children(View)}<View
            tool={kind}
            profile={tabs.profile()?.id ?? "unbound"}
            host="sidebar"
            onclose={toolHost.close}
          />{/snippet}</LazyView
      >{/snippet}</Sidebar
  >
  {#if browserPage.navigationFailed()}<div class="navigation-error" role="alert">
      {m.browser_nav_failed()}
    </div>{/if}
  {#if !tabs.activeTab()?.url && browserPage.currentPage() === null}
    <!--
      Occupies exactly the rect a content WebView would, so moving between a
      page and the new tab never changes the window's shape. The inline start
      inset matches the core layout gap between chrome and content.
    -->
    <main class="min-w-0 flex-1 ps-2" data-zephium-new-tab>
      <div class="content-pane h-full w-full overflow-hidden">
        <NewTab
          clockFormat={preview.get("ntp.clock-format", "System")}
          showGreeting={preview.get("ntp.greeting", true)}
          personalize={preview.get("ntp.personalize", false)}
          showClock={preview.get("ntp.clock", true)}
        />
      </div>
    </main>
  {/if}
  {#if browserPage.currentPage() !== null}
    <main class="internal-stage">
      <RenderBoundary title={m.surface_render_failed()} retryLabel={m.surface_retry()}>
        {#if browserPage.currentPage() === "settings"}
          <LazyView
            loader={loadSettings}
            loadingLabel={m.surface_loading()}
            failureLabel={m.surface_render_failed()}
            retryLabel={m.surface_retry()}>{#snippet children(View)}<View />{/snippet}</LazyView
          >
        {:else}<LibraryPage
            kind={browserPage.currentPage() === "history" ? "history" : "downloads"}
          />{/if}
      </RenderBoundary>
    </main>
  {/if}
</div>

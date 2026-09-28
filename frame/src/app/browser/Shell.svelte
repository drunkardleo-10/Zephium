<script lang="ts">
  const loadWorkWorkspace = () => import("./WorkWorkspace.svelte");
  // Dynamic like the panel's own path to Notes, so the feature's entry is
  // never a startup request.
  const loadNotesPage = () => import("$features/notes").then((notes) => notes.loadNotesPage());
  import { theme } from "$domain/appearance";
  import LazyView from "$shared/ui/LazyView";
  import RenderBoundary from "$shared/ui/RenderBoundary";
  import { loadSettings } from "$features/settings";
  import { EssentialTile } from "$features/essentials";
  import { AddressField } from "$features/address";
  import { Dock } from "$features/dock";
  import { DownloadStatus } from "$features/downloads";
  import { EssentialsRail } from "$features/essentials";
  import { ExtensionActions } from "$features/extensions";
  import { loadWebExtensionManager } from "$features/webext";
  import { sidebarTree } from "$features/tabs";
  import { SidebarBody } from "$features/tabs";
  import { TabList } from "$features/tabs";
  import { TabRail } from "$features/tabs";
  import { selectionGlide } from "$features/tabs";
  import * as tabDrag from "$session/tab-drag.svelte";
  import { expanded as sidebarWidth } from "$session/sidebar-mode.svelte";
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
  import { loadLibraryPage } from "$features/library";
  import { loadHistoryPage } from "$features/history";
  import { loadTasksPage } from "$features/tasks";
  import { loadNewTabSearch } from "$features/search";
  import { loadNewTab } from "$features/newtab";
  import { ModePicker, ModeTabs, Sidebar, UtilityTray } from "$features/sidebar";
  import { IS_MAC } from "$shared/platform";
  import { tabs } from "$domain/tabs";
  /** New Note, from the menu or its shortcut: a note starts where notes are open. */
  async function newNote() {
    const profile = tabs.profile()?.id;
    if (!profile) return;
    const page = browserPage.currentPage() === "notes";
    if (!page) toolHost.open("notes");
    const { noteSession } = await import("$domain/notes");
    await noteSession(profile, page ? "page" : "sidebar")?.create();
  }
  onMount(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void events.uiCommand
      .listen((event) => {
        if (event.payload === "note.new") void newNote();
        else handleNativeSection(event.payload);
      })
      .then((unsubscribe) => {
        if (disposed) unsubscribe();
        else stop = unsubscribe;
      });
    // A note chosen in the launcher opens where notes already are: the page
    // if it is showing, the sidebar otherwise.
    const noteListener = events.noteOpenRequested.listen(({ payload: { profile, id } }) => {
      if (profile !== tabs.profile()?.id) return;
      const page = browserPage.currentPage() === "notes";
      if (!page) toolHost.open("notes");
      void import("$domain/notes").then(async ({ noteSession }) => {
        if (disposed || tabs.profile()?.id !== profile) return;
        await noteSession(profile, page ? "page" : "sidebar")?.requestOpen(id);
      });
    });
    return () => {
      disposed = true;
      stop?.();
      void noteListener.then((stop) => stop());
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

  // Kept sites fill the row beside the tool shelf first; the rest stack in
  // rows of even columns above it. The floor is the narrowest a tile may get
  // before a row gives one up; the lead is the shelf, its rule and their gaps
  // (--dock-tile, --dock-shelf-gap), which the row beside it does not have.
  const TILE_FLOOR = 48;
  const TILE_GAP = 6;
  const SHELF_LEAD = 65;
  let sitesWidth = $state(0);
  // Until the row has been measured, the column's own width stands in for
  // it, so the first frame is already split the way the measured one will be.
  let besideRoom = $derived(sitesWidth || Math.max(0, sidebarWidth() - SHELF_LEAD));
  let beside = $derived(Math.max(1, Math.floor((besideRoom + TILE_GAP) / (TILE_FLOOR + TILE_GAP))));
  let dockColumns = $derived(
    Math.max(1, Math.floor((besideRoom + SHELF_LEAD + TILE_GAP) / (TILE_FLOOR + TILE_GAP))),
  );

  // The current tab's plate travels to the next current tab. Measured before
  // the change lands and played after it, across every list in the column.
  let shownActive: string | null = null;
  let glideFrom: ReturnType<typeof selectionGlide.capture> = null;
  $effect.pre(() => {
    const next = tabs.activeId();
    untrack(() => {
      if (next !== shownActive) glideFrom = selectionGlide.capture(shownActive);
    });
  });
  $effect(() => {
    const next = tabs.activeId();
    untrack(() => {
      if (next === shownActive) return;
      selectionGlide.play(glideFrom, next);
      glideFrom = null;
      shownActive = next;
    });
  });
  let tree = $derived(sidebarTree(tabs.sidebarNodes(), tabs.tabs()));
  let keptBeside = $derived(tree.favorites.slice(0, beside));
  let keptAbove = $derived(tree.favorites.slice(beside));
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
      if (command.id === "extensions.manage") void browserPage.open("extensions");
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
  data-zephium-surface={browserPage.currentPage() ?? "browse"}
>
  <Sidebar
    >{#snippet browserBody(compact)}
      {#if compact}
        <AddressField {compact} />
        {#if toolHost.activeTool() !== null}<ModePicker standalone />{/if}
        <TabRail entries={railTabs} onSelect={selectTab} />
      {:else}
        <!--
          The switch sits above the address field because it governs the
          whole column, field included; the tray rides the field itself,
          because everything in it acts on the page the field names.
        -->
        <div class="sidebar-head"><ModeTabs /></div>
        <AddressField {compact}>
          {#snippet trailing()}
            <UtilityTray>
              <div class="utility-panel">
                <BlockerShield />
                <ExtensionActions />
              </div>
            </UtilityTray>
          {/snippet}
        </AddressField>
        {#if tabs.profile()?.id}<DownloadStatus
            profile={tabs.profile()!.id}
            onopen={() => toolHost.open("downloads")}
          />{/if}
        {#if splitting}<p class="shrink-0 px-3 pb-1 text-[12px] text-accent" aria-live="polite">
            {m.choose_split()}
          </p>{/if}
        <SidebarBody pinned={tree.pinned} today={tree.today} {splitting} onSelect={selectTab} />
      {/if}
    {/snippet}{#snippet dock(compact)}{#if compact}<Dock compact>
          {#snippet sites()}<EssentialsRail
              entries={railEssentials}
              onSelect={selectTab}
            />{/snippet}
        </Dock>{:else}<Dock>
          {#snippet above()}
            <div
              class="dock-sites"
              data-essentials-drop
              data-over={tabDrag.overEssentials()}
              hidden={keptAbove.length === 0}
            >
              <TabList
                entries={keptAbove}
                section="favorites"
                variant="essentials"
                columns={dockColumns}
                label={m.essentials()}
                {splitting}
                onSelect={selectTab}
                >{#snippet essentialTile(props)}<EssentialTile {...props} />{/snippet}</TabList
              >
            </div>
          {/snippet}
          {#snippet sites()}
            <div
              class="dock-sites"
              bind:clientWidth={sitesWidth}
              data-essentials-drop
              data-over={tabDrag.overEssentials()}
              data-empty={tree.favorites.length === 0}
            >
              {#if tree.favorites.length === 0}<span class="dock-sites-hint"
                  >{m.essential_drop_hint()}</span
                >{/if}
              <TabList
                entries={keptBeside}
                section="favorites"
                variant="essentials"
                label={m.essentials()}
                {splitting}
                onSelect={selectTab}
                >{#snippet essentialTile(props)}<EssentialTile {...props} />{/snippet}</TabList
              >
            </div>
            {#if tabDrag.moveFailed()}<p class="sidebar-move-error" role="alert">
                {m.essential_move_failed()}
              </p>{/if}
          {/snippet}
        </Dock>{/if}{/snippet}{#snippet settingsNavigation()}<SettingsNavigation
      />{/snippet}{#snippet toolPanel(kind)}<LazyView
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
  {#if !tabs.activeTab()?.url && (tabs.activeTab()?.content ?? "web") === "web" && browserPage.currentPage() === null}
    <!--
      Occupies exactly the rect a content WebView would, so moving between a
      page and the new tab never changes the window's shape. The inline start
      inset matches the core layout gap between chrome and content.
    -->
    <main class="min-w-0 flex-1 ps-2" data-zephium-new-tab>
      <div class="content-pane h-full w-full overflow-hidden">
        <LazyView
          loader={loadNewTab}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(NewTab)}<NewTab
              clockFormat={preview.get("ntp.clock-format", "System")}
              showGreeting={preview.get("ntp.greeting", true)}
              personalize={preview.get("ntp.personalize", false)}
              showClock={preview.get("ntp.clock", true)}
              >{#snippet search()}{#key tabs.activeId()}<LazyView
                    loader={loadNewTabSearch}
                    loadingLabel={m.surface_loading()}
                    failureLabel={m.surface_render_failed()}
                    retryLabel={m.surface_retry()}
                    >{#snippet children(Search)}<Search
                        tabId={tabs.activeId()}
                      />{/snippet}</LazyView
                  >{/key}{/snippet}</NewTab
            >{/snippet}</LazyView
        >
      </div>
    </main>
  {/if}
  {#if browserPage.currentPage() !== null}
    <main class="internal-stage">
      <!-- Each destination arrives as its own page: the ground settles in
           while its content rises onto it. -->
      {#key browserPage.currentPage()}<div class="stage-page">
          <RenderBoundary title={m.surface_render_failed()} retryLabel={m.surface_retry()}>
            {#if browserPage.currentPage() === "settings"}
              <LazyView
                loader={loadSettings}
                loadingLabel={m.surface_loading()}
                failureLabel={m.surface_render_failed()}
                retryLabel={m.surface_retry()}>{#snippet children(View)}<View />{/snippet}</LazyView
              >
            {:else if browserPage.currentPage() === "extensions"}
              <LazyView
                loader={loadWebExtensionManager}
                loadingLabel={m.surface_loading()}
                failureLabel={m.surface_render_failed()}
                retryLabel={m.surface_retry()}>{#snippet children(View)}<View />{/snippet}</LazyView
              >
            {:else if browserPage.currentPage() === "work"}
              {#key tabs.profile()?.id}<LazyView
                  loader={loadWorkWorkspace}
                  loadingLabel={m.surface_loading()}
                  failureLabel={m.surface_render_failed()}
                  retryLabel={m.surface_retry()}
                  >{#snippet children(View)}<View />{/snippet}</LazyView
                >{/key}
            {:else if browserPage.currentPage() === "notes"}
              {#key tabs.profile()?.id}<LazyView
                  loader={loadNotesPage}
                  loadingLabel={m.surface_loading()}
                  failureLabel={m.surface_render_failed()}
                  retryLabel={m.surface_retry()}
                  >{#snippet children(View)}<View
                      profile={tabs.profile()?.id ?? ""}
                      onclose={() => void browserPage.open(null)}
                    />{/snippet}</LazyView
                >{/key}
            {:else if browserPage.currentPage() === "tasks"}
              <LazyView
                loader={loadTasksPage}
                loadingLabel={m.surface_loading()}
                failureLabel={m.surface_render_failed()}
                retryLabel={m.surface_retry()}>{#snippet children(View)}<View />{/snippet}</LazyView
              >
            {:else if browserPage.currentPage() === "history"}
              <LazyView
                loader={loadHistoryPage}
                loadingLabel={m.surface_loading()}
                failureLabel={m.surface_render_failed()}
                retryLabel={m.surface_retry()}>{#snippet children(View)}<View />{/snippet}</LazyView
              >
            {:else}
              <LazyView
                loader={loadLibraryPage}
                loadingLabel={m.surface_loading()}
                failureLabel={m.surface_render_failed()}
                retryLabel={m.surface_retry()}
                >{#snippet children(View)}<View kind="downloads" />{/snippet}</LazyView
              >{/if}
          </RenderBoundary>
        </div>{/key}
    </main>
  {/if}
</div>

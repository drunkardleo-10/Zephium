<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { Snippet } from "svelte";
  import * as motion from "$session/motion.svelte";
  import { surface as browserPage } from "$domain/surface";
  import * as tools from "$session/tools.svelte";
  import { untrack } from "svelte";
  import {
    COMPACT_WIDTH,
    effectiveWidth,
    isCompact,
    toggleMode,
  } from "$session/sidebar-mode.svelte";
  import { uiCommands as ui } from "$domain/ui-commands";
  import SidebarHeader from "./SidebarHeader.svelte";
  import SidebarResizeHandle from "./SidebarResizeHandle.svelte";

  let {
    settingsNavigation,
    toolPanel,
    browserBody,
    dock,
  }: {
    settingsNavigation: Snippet;
    toolPanel: Snippet<[tools.ToolKind]>;
    browserBody: Snippet<[boolean]>;
    dock: Snippet<[boolean]>;
  } = $props();
  let settings = $derived(browserPage.currentPage() === "settings");
  let taskPage = $derived(browserPage.currentPage() === "tasks");
  // Settings takes the column for its own navigation. Tasks is part of
  // browsing, so the column stays as the tab rail: a tab is one click away and
  // choosing it returns to that page.
  let compact = $derived(!settings && (taskPage || isCompact() || tools.activeTool() !== null));
  let width = $derived(
    settings ? 240 : taskPage && tools.activeTool() === null ? COMPACT_WIDTH : effectiveWidth(),
  );

  // Dispatch runs untracked and behind a sequence guard. Handlers read the
  // state they mutate (the sidebar shape, the tab projection), so a tracked
  // effect would subscribe to its own writes and re-fire on the same command,
  // which made the collapsed menu's Compact Mode toggle immediately undo
  // itself.
  let handledCommand = 0;
  $effect(() => {
    const command = ui.uiCommand();
    if (command.seq === 0 || command.seq === handledCommand) return;
    handledCommand = command.seq;

    untrack(() => {
      if (command.id === "sidebar.toggleCompact") toggleShape();
    });
  });

  function toggleShape() {
    if (tools.activeTool() !== null) tools.close();
    else toggleMode();
  }
</script>

<aside
  data-tauri-drag-region="deep"
  aria-label={m.ui_browser_sidebar()}
  style:width={`${width}px`}
  style:--sidebar-width={`${width}px`}
  class="browser-sidebar relative flex shrink-0 flex-col text-text select-none"
>
  {#if !settings && !taskPage && tools.activeTool() === null}<SidebarResizeHandle {width} />{/if}
  <SidebarHeader
    compact={compact && tools.activeTool() === null}
    launcher={!settings && tools.activeTool() !== null}
    ontoggle={toggleShape}
    navigation={!settings}
  />
  {#if settings}
    {@render settingsNavigation()}
  {:else}
    <div class="sidebar-columns" data-launch={motion.launchActive()}>
      <div class="sidebar-browser-column" class:sidebar-tool-rail={tools.activeTool() !== null}>
        {@render browserBody(compact)}
        {@render dock(compact)}
      </div>
      {#if tools.activeTool() !== null}<div class="sidebar-tool-host">
          {@render toolPanel(tools.activeTool()!)}
        </div>{/if}
    </div>
  {/if}
</aside>

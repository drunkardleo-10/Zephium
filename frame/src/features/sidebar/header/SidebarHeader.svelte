<script lang="ts">
  import {
    ArrowLeft02Icon,
    ArrowRight02Icon,
    MoreHorizontalIcon,
    Refresh01Icon,
    ViewSidebarLeftIcon,
  } from "@hugeicons/core-free-icons";
  import * as tabs from "../../../domain/tabs/tabs.svelte";
  import { commands } from "../../../shared/ipc/bindings";
  import { IS_MAC } from "../../../shared/platform";
  import IconButton from "../../../shared/ui/IconButton.svelte";
  import { toggleMode } from "../sidebar-mode.svelte";
  import WindowControls from "./WindowControls.svelte";

  let { compact }: { compact: boolean } = $props();

  let active = $derived(tabs.activeTab());

  function openSidebarMenu(event: MouseEvent) {
    const target = event.currentTarget;
    if (!(target instanceof HTMLButtonElement)) return;
    const anchor = target.getBoundingClientRect();
    void commands.sidebarMenuPopup(anchor.left, anchor.bottom);
  }
</script>

<!--
  macOS keeps AppKit's traffic lights through the overlay title bar, so the
  leading 52px of the top row belongs to the system. Elsewhere the window
  controls are ours, and at rail width they move into the collapsed menu
  because three buttons cannot fit beside anything else.
-->
{#if compact}
  <header class="flex shrink-0 flex-col items-center" aria-label="Navigation">
    {#if IS_MAC}
      <div style:height="var(--titlebar-height)" aria-hidden="true"></div>
    {/if}
    <IconButton
      icon={MoreHorizontalIcon}
      label="Sidebar options"
      size={17}
      haspopup
      onclick={openSidebarMenu}
    />
  </header>
{:else}
  <header
    class="flex shrink-0 items-center gap-px pe-1.5"
    style:height="var(--titlebar-height)"
    style:padding-inline-start={IS_MAC ? "var(--traffic-light-inset)" : "6px"}
    aria-label="Navigation"
  >
    <IconButton icon={ViewSidebarLeftIcon} label="Compact Mode" onclick={toggleMode} />

    <span class="flex-1" aria-hidden="true"></span>

    <IconButton
      icon={ArrowLeft02Icon}
      label="Back"
      disabled={active?.can_go_back !== true}
      onclick={tabs.backActive}
    />
    <IconButton
      icon={ArrowRight02Icon}
      label="Forward"
      disabled={active?.can_go_forward !== true}
      onclick={tabs.forwardActive}
    />
    <IconButton
      icon={Refresh01Icon}
      label="Reload"
      disabled={active === undefined}
      onclick={tabs.reloadActive}
    />

    {#if !IS_MAC}
      <WindowControls />
    {/if}
  </header>
{/if}

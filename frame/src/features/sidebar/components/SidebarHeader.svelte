<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import {
    ArrowLeft02Icon,
    ArrowRight02Icon,
    MoreHorizontalIcon,
    Refresh01Icon,
    ViewSidebarLeftIcon,
  } from "@hugeicons/core-free-icons";
  import { tabs } from "$domain/tabs";
  import { commands } from "$shared/ipc/bindings";
  import { IS_MAC } from "$shared/platform";
  import IconButton from "$shared/ui/IconButton";
  import WindowControls from "./WindowControls.svelte";

  let {
    compact,
    ontoggle,
    navigation = true,
  }: { compact: boolean; ontoggle: () => void; navigation?: boolean } = $props();

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
  <header class="flex shrink-0 flex-col items-center" aria-label={m.ui_navigation()}>
    {#if IS_MAC}
      <div style:height="var(--titlebar-height)" aria-hidden="true"></div>
    {/if}
    <IconButton
      icon={MoreHorizontalIcon}
      label={m.ui_sidebar_options()}
      size={16}
      haspopup
      onclick={openSidebarMenu}
    />
  </header>
{:else}
  <header
    class="flex shrink-0 items-center gap-px pe-1.5"
    style:height="var(--titlebar-height)"
    style:padding-inline-start={IS_MAC ? "var(--traffic-light-inset)" : "6px"}
    aria-label={m.ui_navigation()}
  >
    {#if navigation}<IconButton
        icon={ViewSidebarLeftIcon}
        label={m.ui_compact_mode()}
        onclick={ontoggle}
      />
    {/if}<span class="flex-1" aria-hidden="true"></span>

    {#if navigation}<div
        class="flex items-center gap-0.5"
        role="group"
        aria-label={m.ui_navigation()}
      >
        <IconButton
          icon={ArrowLeft02Icon}
          label={m.ui_back()}
          shape="rounded"
          buttonSize={28}
          size={16}
          disabled={active?.can_go_back !== true}
          onclick={tabs.backActive}
        />
        <IconButton
          icon={ArrowRight02Icon}
          label={m.ui_forward()}
          shape="rounded"
          buttonSize={28}
          size={16}
          disabled={active?.can_go_forward !== true}
          onclick={tabs.forwardActive}
        />
        <IconButton
          icon={Refresh01Icon}
          label={m.ui_reload()}
          shape="rounded"
          buttonSize={28}
          size={16}
          disabled={active === undefined}
          onclick={tabs.reloadActive}
        />
      </div>{/if}

    {#if !IS_MAC}
      <WindowControls />
    {/if}
  </header>
{/if}

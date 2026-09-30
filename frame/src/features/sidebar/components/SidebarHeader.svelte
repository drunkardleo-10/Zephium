<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import {
    ArrowLeft02Icon,
    ArrowRight02Icon,
    EllipsisIcon,
    Refresh01Icon,
    Search01Icon,
    SidebarLeftIcon,
  } from "@hugeicons/core-free-icons";
  import { tabs } from "$domain/tabs";
  import { blocker, siteMenuState } from "$domain/blocker";
  import { commands } from "$shared/ipc/bindings";
  import { IS_MAC } from "$shared/platform";
  import IconButton from "$shared/ui/IconButton";
  import ModePicker from "./ModePicker.svelte";
  import WindowControls from "./WindowControls.svelte";

  let {
    compact,
    ontoggle,
    navigation = true,
    launcher = false,
  }: {
    compact: boolean;
    ontoggle: () => void;
    navigation?: boolean;
    /** The rail beside a tool panel has no head of its own to carry search. */
    launcher?: boolean;
  } = $props();

  let active = $derived(tabs.activeTab());

  function openSidebarMenu(event: MouseEvent) {
    const target = event.currentTarget;
    if (!(target instanceof HTMLButtonElement)) return;
    const anchor = target.getBoundingClientRect();
    const { siteProtected, canHide } = siteMenuState(blocker.status());
    void commands.sidebarMenuPopup(anchor.left, anchor.bottom, siteProtected, canHide);
  }
</script>

<!--
  macOS keeps AppKit's traffic lights through the overlay title bar, so the
  leading 52px of the top row belongs to the system. Elsewhere the window
  controls are ours, and at rail width they move into the collapsed menu
  because three buttons cannot fit beside anything else.
-->
{#if compact}
  <!--
    The rail's head: which environment you are in, then the two controls that
    act on the column. At 56px two 26px buttons are exactly what fits on one
    line, and the rule below them says where the head ends.
  -->
  <header class="flex shrink-0 flex-col items-center gap-[3px]" aria-label={m.ui_navigation()}>
    {#if IS_MAC}
      <div style:height="var(--titlebar-height)" aria-hidden="true"></div>
    {/if}
    <ModePicker />
    <div class="flex items-center gap-0.5">
      <IconButton
        icon={EllipsisIcon}
        label={m.ui_sidebar_options()}
        size={15}
        buttonSize={26}
        haspopup
        onclick={openSidebarMenu}
      />
      <IconButton
        icon={Search01Icon}
        label={m.ui_search_or_enter_an_address()}
        size={15}
        buttonSize={26}
        onclick={() => void commands.runCommand("launcher.toggle")}
      />
    </div>
    <span class="mt-[6px] mb-[7px] h-px w-[22px] rounded-full bg-border" aria-hidden="true"></span>
  </header>
{:else}
  <header
    class="flex shrink-0 items-center gap-px pe-1.5"
    style:height="var(--titlebar-height)"
    style:padding-inline-start={IS_MAC ? "var(--traffic-light-inset)" : "6px"}
    aria-label={m.ui_navigation()}
  >
    {#if navigation}<IconButton
        icon={SidebarLeftIcon}
        label={m.ui_compact_mode()}
        onclick={ontoggle}
      />
    {/if}{#if launcher}<IconButton
        icon={Search01Icon}
        label={m.ui_search_or_enter_an_address()}
        onclick={() => void commands.runCommand("launcher.toggle")}
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

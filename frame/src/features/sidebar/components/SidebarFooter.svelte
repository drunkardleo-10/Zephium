<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import {
    BellIcon,
    Cancel01Icon,
    IncognitoIcon,
    MoreHorizontalIcon,
  } from "@hugeicons/core-free-icons";
  import ModeSwitch from "./ModeSwitch.svelte";
  import { commands } from "$shared/ipc/bindings";
  import { runtimeNotifications } from "$domain/runtime";
  import { runtime } from "$domain/runtime";
  import { tabs } from "$domain/tabs";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";

  let { compact = false, showMode = true }: { compact?: boolean; showMode?: boolean } = $props();

  let profile = $derived(tabs.profile());
  let name = $derived(profile?.name ?? "Personal");
  let incognito = $derived(profile?.kind === "incognito");
  let notifications = $derived(runtimeNotifications(runtime.status()));
  let hasWarning = $derived(notifications.some((notification) => notification.tone === "warning"));
  let notificationsOpen = $state(false);
  let footer: HTMLElement;
  let bellAnchor = $state<HTMLSpanElement>();
  let closeButton = $state<HTMLButtonElement>();

  $effect(() => {
    if (notifications.length === 0) notificationsOpen = false;
  });

  function toggleNotifications() {
    notificationsOpen = !notificationsOpen;
    if (notificationsOpen) {
      queueMicrotask(() => closeButton?.focus());
    }
  }

  function closeNotifications(returnFocus = false) {
    notificationsOpen = false;
    if (returnFocus) {
      queueMicrotask(() => bellAnchor?.querySelector<HTMLButtonElement>("button")?.focus());
    }
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (!notificationsOpen) return;
    if (event.key === "Escape") {
      event.preventDefault();
      closeNotifications(true);
    } else if (event.key === "Tab") {
      // The current dialog has one interactive control. Keep keyboard focus
      // inside it until Escape, Close, or an outside pointer action dismisses
      // the popover.
      event.preventDefault();
      closeButton?.focus();
    }
  }

  function handleWindowPointerDown(event: PointerEvent) {
    if (!notificationsOpen || !(event.target instanceof Node) || footer.contains(event.target)) {
      return;
    }
    closeNotifications();
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} onpointerdown={handleWindowPointerDown} />

<!--
  The profile is a security boundary, so it reads as identity rather than as
  another switcher chip. Its menu is everything that is yours: account,
  profiles, your notes and tasks, appearance, settings.
-->
<footer bind:this={footer} class="sidebar-footer" class:sidebar-footer-compact={compact}>
  {#if notificationsOpen && notifications.length > 0}
    <div
      id="runtime-notifications"
      role="dialog"
      aria-modal="true"
      aria-labelledby="runtime-notifications-title"
      class="absolute right-1.5 bottom-full left-1.5 z-20 mb-1.5 rounded-lg bg-raised p-2.5 shadow-[var(--shadow-popover)]"
    >
      <div class="mb-1.5 flex items-center justify-between gap-2">
        <h2 id="runtime-notifications-title" class="text-[13px] font-medium text-text">
          Notifications
        </h2>
        <button
          bind:this={closeButton}
          type="button"
          aria-label="Close notifications"
          class="icon-button"
          style:--icon-button-size="24px"
          onclick={() => closeNotifications(true)}
        >
          <Icon icon={Cancel01Icon} size={14} />
        </button>
      </div>

      <div class="space-y-1.5">
        {#each notifications as notification (notification.id)}
          <article class="rounded-md bg-fill px-2.5 py-2">
            <div class="flex items-start gap-2">
              <span
                class="mt-1 h-1.5 w-1.5 shrink-0 rounded-full"
                class:bg-warning={notification.tone === "warning"}
                class:bg-info={notification.tone === "info"}
              ></span>
              <div class="min-w-0">
                <h3 class="text-[12.5px] leading-4 font-medium text-text">
                  {notification.title}
                </h3>
                <p class="mt-0.5 text-[11.5px] leading-[16px] text-muted">
                  {notification.detail}
                </p>
              </div>
            </div>
          </article>
        {/each}
      </div>
    </div>
  {/if}

  <div class="sidebar-footer-row">
    <button
      type="button"
      class="profile-menu-button browser-menu-button"
      aria-label={m.browser_menu_profile({ profile: name })}
      title={m.browser_menu_profile({ profile: name })}
      aria-haspopup="menu"
      onclick={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        void commands.profileMenuPopup(rect.left, rect.top);
      }}
    >
      <Icon icon={incognito ? IncognitoIcon : MoreHorizontalIcon} size={20} />
    </button>
    {#if showMode}<ModeSwitch {compact} />{/if}
  </div>
  {#if notifications.length > 0 && !compact}
    <span bind:this={bellAnchor} class="sidebar-notification-anchor">
      <IconButton
        icon={BellIcon}
        label={`Notifications (${notifications.length})`}
        size={16}
        active={notificationsOpen}
        haspopup="dialog"
        expanded={notificationsOpen}
        controls="runtime-notifications"
        onclick={toggleNotifications}
      />
      <span
        aria-hidden="true"
        class="pointer-events-none absolute top-1 right-1 h-1.5 w-1.5 rounded-full ring-2 ring-chrome"
        class:bg-warning={hasWarning}
        class:bg-info={!hasWarning}
      ></span>
    </span>
  {/if}
</footer>

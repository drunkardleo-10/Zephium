<script lang="ts">
  import {
    Add01Icon,
    BellIcon,
    Cancel01Icon,
    IncognitoIcon,
    UserCircleIcon,
  } from "@hugeicons/core-free-icons";
  import { commands } from "../../../shared/ipc/bindings";
  import { runtimeNotifications } from "../../../domain/runtime/runtime-model";
  import * as runtime from "../../../domain/runtime/runtime.svelte";
  import * as tabs from "../../../domain/tabs/tabs.svelte";
  import Icon from "../../../shared/ui/Icon.svelte";
  import IconButton from "../../../shared/ui/IconButton.svelte";

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

  function anchorOf(event: MouseEvent): DOMRect | null {
    const target = event.currentTarget;
    if (!(target instanceof HTMLButtonElement)) return null;
    return target.getBoundingClientRect();
  }

  function openProfileMenu(event: MouseEvent) {
    notificationsOpen = false;
    const anchor = anchorOf(event);
    if (anchor !== null) void commands.profileMenuPopup(anchor.left, anchor.top);
  }

  function openAddMenu(event: MouseEvent) {
    notificationsOpen = false;
    const anchor = anchorOf(event);
    if (anchor !== null) void commands.addMenuPopup(anchor.left, anchor.top, tabs.canSplitActive());
  }

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
  another switcher chip. On Windows and Linux this menu is also the app menu.
-->
<footer bind:this={footer} class="relative shrink-0 border-t border-border px-1.5 pt-1.5 pb-1.5">
  {#if notificationsOpen && notifications.length > 0}
    <div
      id="runtime-notifications"
      role="dialog"
      aria-modal="true"
      aria-labelledby="runtime-notifications-title"
      class="absolute right-1.5 bottom-full left-1.5 z-20 mb-1.5 rounded-lg border border-border-strong bg-raised p-2.5 shadow-[var(--shadow-overlay)]"
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

  <div class="flex h-8 items-center gap-1">
    <button
      type="button"
      aria-label={`Profile: ${name}`}
      aria-haspopup="menu"
      title={name}
      class="flex h-[34px] min-w-0 flex-1 items-center gap-2 rounded-md px-2 text-start text-[13.5px] text-muted transition-[background-color,color] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none hover:bg-fill-hover hover:text-text"
      onclick={openProfileMenu}
    >
      <span
        class="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full bg-fill"
        class:text-accent={incognito}
      >
        <Icon icon={incognito ? IncognitoIcon : UserCircleIcon} size={15} />
      </span>
      <span class="min-w-0 flex-1 truncate">{name}</span>
    </button>

    {#if notifications.length > 0}
      <span bind:this={bellAnchor} class="relative flex shrink-0">
        <IconButton
          icon={BellIcon}
          label={`Notifications (${notifications.length})`}
          size={17}
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

    <IconButton
      icon={Add01Icon}
      label="New tab and split options"
      size={18}
      haspopup
      onclick={openAddMenu}
    />
  </div>
</footer>

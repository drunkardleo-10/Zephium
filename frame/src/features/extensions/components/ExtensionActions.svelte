<script lang="ts">
  import { extensions } from "$domain/extensions";
  import { tabs } from "$domain/tabs";
  import ExtensionActionIcon from "./ExtensionActionIcon.svelte";
  import ExtensionManager from "./ExtensionManager.svelte";

  let { compact = false }: { compact?: boolean } = $props();

  let profileId = $derived(tabs.profile()?.id ?? null);
  let tabId = $derived(tabs.activeId());
  let actions = $derived(extensions.activeActions(profileId, tabId));
  let failure = $derived(extensions.failureReason(profileId, tabId));
  let shortcut = $derived(extensions.actionShortcut(profileId, tabId));
  let actionRoot = $state<HTMLDivElement>();

  const failureMessage = (reason: ReturnType<typeof extensions.failureReason>) => {
    switch (reason) {
      case "tab_discarded":
        return "This tab is sleeping. Activate it before opening the extension.";
      case "action_disabled":
        return "This extension action is disabled on the current page.";
      case "popup_capacity_exceeded":
      case "capacity_exceeded":
        return "Close the current extension popup and try again.";
      case "unsupported_platform":
        return "This extension action is not supported on this platform.";
      case "shutting_down":
        return "Extensions are unavailable while Zephium is closing.";
      case "runtime_superseded":
      case "runtime_unavailable":
      case "tab_unavailable":
      case "action_unavailable":
        return "The extension changed. Try the action again.";
      case "popup_unavailable":
        return "This extension popup is unavailable.";
      case "invalid_request":
      case "native_admission_failed":
        return "Zephium couldn't open this extension action.";
      case null:
        return "";
    }
  };

  function invoke(event: MouseEvent, action: (typeof actions)[number]) {
    const target = event.currentTarget;
    if (profileId === null || tabId === null || !(target instanceof HTMLButtonElement)) return;
    void extensions.invoke(profileId, tabId, action, target.getBoundingClientRect());
  }

  $effect(() => {
    const request = shortcut;
    const root = actionRoot;
    if (request === null || root === undefined || profileId === null || tabId === null) return;
    const action = actions.find(
      (candidate) =>
        candidate.runtime.install_id === request.runtime.install_id &&
        candidate.runtime.generation === request.runtime.generation &&
        candidate.revision === request.action_revision,
    );
    if (action === undefined || !action.enabled) return;
    const button = [
      ...root.querySelectorAll<HTMLButtonElement>("button[data-extension-install]"),
    ].find((candidate) => candidate.dataset.extensionInstall === request.runtime.install_id);
    if (button === undefined || button.disabled) return;
    if (!extensions.consumeActionShortcut(request.projection_revision)) return;
    void extensions.invoke(profileId, tabId, action, button.getBoundingClientRect());
  });
</script>

<div
  bind:this={actionRoot}
  class="relative flex shrink-0 gap-px px-1.5 pb-1.5"
  class:flex-wrap={!compact}
  class:flex-col={compact}
  class:items-center={compact}
  aria-label="Extensions"
  data-zephium-extension-actions
>
  {#if actions.length > 0}
    {#each actions as action (action.runtime.install_id)}
      <button
        type="button"
        aria-label={action.label || "Extension action"}
        aria-haspopup={action.presents_popup ? "dialog" : undefined}
        title={action.label || "Extension action"}
        disabled={!action.enabled || extensions.isInvoking(action.runtime.install_id)}
        data-extension-install={action.runtime.install_id}
        class="icon-button relative"
        style:--icon-button-size="28px"
        onclick={(event) => invoke(event, action)}
      >
        <ExtensionActionIcon rgba={action.icon_rgba_base64} />
        {#if action.badge.length > 0}
          <span
            aria-hidden="true"
            class="pointer-events-none absolute -top-0.5 -right-1 max-w-[30px] truncate rounded-full px-1 text-[8px] leading-[13px] font-semibold text-chrome ring-1 ring-chrome"
            class:bg-warning={action.unread_badge}
            class:bg-accent={!action.unread_badge}
          >
            {action.badge}
          </span>
        {/if}
      </button>
    {/each}
  {/if}

  <ExtensionManager {compact} />

  {#if failure !== null}
    {#if compact}
      <p role="status" aria-live="polite" class="sr-only">{failureMessage(failure)}</p>
    {:else}
      <p
        role="status"
        aria-live="polite"
        class="absolute top-full right-1.5 left-1.5 z-20 rounded-control border border-border-strong bg-raised px-2.5 py-2 text-[11.5px] leading-4 text-text shadow-[var(--shadow-overlay)]"
      >
        {failureMessage(failure)}
      </p>
    {/if}
  {/if}
</div>

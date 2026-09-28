<script lang="ts">
  import { extensions } from "$domain/extensions";
  import { tabs } from "$domain/tabs";
  import * as m from "$shared/i18n/messages";
  import ExtensionActionIcon from "./ExtensionActionIcon.svelte";

  let { variant = "grid" }: { variant?: "grid" | "stack" } = $props();

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

<!--
  The extensions' own buttons. In the tray they are a grid of tiles named by
  their tooltips; stacked in the rail's tool case they are glyph rows like
  the tools beside them. Either way they are items of the menu around them,
  so choosing one closes it and arrow keys travel over them.
-->
<div
  bind:this={actionRoot}
  class="actions"
  data-variant={variant}
  aria-label={m.webext_page_title()}
  data-zephium-extension-actions
>
  {#each actions as action (action.runtime.install_id)}
    <button
      type="button"
      role="menuitem"
      aria-label={action.label || m.webext_action()}
      aria-haspopup={action.presents_popup ? "dialog" : undefined}
      title={action.label || m.webext_action()}
      disabled={!action.enabled || extensions.isInvoking(action.runtime.install_id)}
      data-extension-install={action.runtime.install_id}
      class={variant === "stack" ? "ui-menu-item shelf-item tile" : "tile"}
      onclick={(event) => invoke(event, action)}
    >
      <span class="glyph">
        <ExtensionActionIcon rgba={action.icon_rgba_base64} />
        {#if action.badge.length > 0}
          <span aria-hidden="true" class="badge" data-unread={action.unread_badge}>
            {action.badge}
          </span>
        {/if}
      </span>
    </button>
  {/each}
</div>

{#if failure !== null}
  <p role="status" aria-live="polite" class="failure" class:sr-only={variant === "stack"}>
    {failureMessage(failure)}
  </p>
{/if}

<style>
  .actions[data-variant="grid"] {
    display: grid;
    grid-template-columns: repeat(5, 36px);
    gap: 4px;
  }

  .actions[data-variant="grid"]:empty {
    display: none;
  }

  .actions[data-variant="grid"] .tile {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: none;
    cursor: default;
    outline: none;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  .actions[data-variant="grid"] .tile:hover,
  .actions[data-variant="grid"] .tile:focus-visible {
    background: var(--row-hover);
  }

  .actions[data-variant="grid"] .tile:active {
    background: var(--row-pressed);
  }

  .actions[data-variant] .tile:disabled {
    opacity: 0.45;
  }

  .glyph {
    position: relative;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
  }

  .glyph :global(canvas) {
    width: 20px;
    height: 20px;
  }

  .badge {
    position: absolute;
    top: -5px;
    right: -8px;
    max-width: 30px;
    overflow: hidden;
    padding-inline: 4px;
    border-radius: var(--radius-capsule);
    background: var(--color-accent);
    box-shadow: 0 0 0 1.5px var(--color-menu);
    font-size: 9px;
    font-weight: 600;
    line-height: 14px;
    color: var(--color-on-accent);
    white-space: nowrap;
    text-overflow: ellipsis;
    pointer-events: none;
  }

  .badge[data-unread="true"] {
    background: var(--color-warning);
  }

  .failure {
    max-width: 196px;
    margin: 8px 2px 0;
    font-size: var(--text-label);
    line-height: 1.4;
    color: var(--color-muted);
  }
</style>

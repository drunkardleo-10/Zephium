<script lang="ts">
  import { Shield01Icon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import type { ExtensionRuntimeGrantPromptEntryView } from "$shared/ipc/bindings";
  import { extensions } from "$domain/extensions";
  import { apiPermissionLabel, hostPermissionLabel } from "$domain/extensions";
  import Icon from "$shared/ui/Icon";
  import Button from "$shared/ui/Button";

  let { prompt }: { prompt: ExtensionRuntimeGrantPromptEntryView } = $props();
  let denyButton = $state<HTMLButtonElement>();
  let allowButton = $state<HTMLButtonElement>();
  let busy = $derived(extensions.permissionPromptBusy());
  let failure = $derived(extensions.permissionPromptFailure());

  onMount(() => {
    // Consent never receives an affirmative default. Keyboard focus begins
    // on the safe action while both choices remain equally visible.
    queueMicrotask(() => denyButton?.focus());
  });

  function respond(allow: boolean) {
    if (busy) return;
    extensions.respondToPermissionPrompt(prompt, allow);
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (!busy) respond(false);
      return;
    }
    if (event.key !== "Tab") return;
    event.stopPropagation();
    if (denyButton === undefined || allowButton === undefined) return;
    if (event.shiftKey && document.activeElement === denyButton) {
      event.preventDefault();
      allowButton.focus();
    } else if (!event.shiftKey && document.activeElement === allowButton) {
      event.preventDefault();
      denyButton.focus();
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-canvas/80 p-4"
  data-zephium-extension-permission-prompt
>
  <div
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="extension-permission-title"
    aria-describedby="extension-permission-summary extension-permission-access"
    class="max-h-[min(560px,calc(100vh-32px))] w-[min(390px,calc(100vw-32px))] overflow-y-auto rounded-xl border border-border-strong bg-raised p-4 text-start shadow-[var(--shadow-overlay)]"
  >
    <div class="flex items-start gap-3">
      <span
        class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent"
        aria-hidden="true"
      >
        <Icon icon={Shield01Icon} size={19} />
      </span>
      <div class="min-w-0 flex-1">
        <h1 id="extension-permission-title" class="text-[14px] leading-5 font-semibold text-text">
          Allow additional access?
        </h1>
        <p id="extension-permission-summary" class="mt-0.5 text-[11.5px] leading-4 text-muted">
          <span class="font-medium text-text">{prompt.extension_name}</span> is requesting new permissions.
        </p>
      </div>
    </div>

    <div id="extension-permission-access" class="mt-3 rounded-lg bg-fill px-3 py-2.5">
      <p class="text-[10.5px] leading-4 font-medium tracking-wide text-faint uppercase">
        Requested access
      </p>
      <ul class="mt-1.5 space-y-1.5 text-[11.5px] leading-4 text-text">
        {#each prompt.api_permissions as permission (permission)}
          <li class="flex items-start gap-2">
            <span class="mt-[6px] h-1 w-1 shrink-0 rounded-full bg-accent"></span>
            <span>{apiPermissionLabel(permission)}</span>
          </li>
        {/each}
        {#each prompt.host_permissions as pattern (pattern)}
          <li class="flex items-start gap-2">
            <span class="mt-[6px] h-1 w-1 shrink-0 rounded-full bg-accent"></span>
            <span class="break-words">{hostPermissionLabel(pattern)}</span>
          </li>
        {/each}
      </ul>
    </div>

    {#if prompt.private_context}
      <p class="mt-2.5 rounded-md bg-fill px-2.5 py-2 text-[10.5px] leading-4 text-muted">
        This access applies only to this extension's Private Browsing context.
      </p>
    {/if}

    {#if failure !== null}
      <p role="alert" class="mt-2.5 text-[10.5px] leading-4 text-danger">{failure}</p>
    {:else if busy}
      <p role="status" class="mt-2.5 text-[10.5px] leading-4 text-muted">
        Applying the permission securely…
      </p>
    {/if}

    <div class="mt-4 flex justify-end gap-2">
      <Button
        bind:ref={denyButton}
        variant="secondary"
        disabled={busy}
        aria-busy={busy || undefined}
        onclick={() => respond(false)}
      >
        Deny
      </Button>
      <Button
        bind:ref={allowButton}
        variant="primary"
        disabled={busy}
        aria-busy={busy || undefined}
        onclick={() => respond(true)}
      >
        Allow
      </Button>
    </div>
  </div>
</div>

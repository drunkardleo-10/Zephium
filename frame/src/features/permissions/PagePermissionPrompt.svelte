<script lang="ts">
  import { Camera01Icon, Mic01Icon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import type {
    PagePermissionPromptDecisionInput,
    PagePermissionPromptEntryView,
  } from "../../shared/ipc/bindings";
  import * as permissions from "../../domain/permissions/page-permissions.svelte";
  import Icon from "../../shared/ui/Icon.svelte";

  let { prompt }: { prompt: PagePermissionPromptEntryView } = $props();
  let remember = $state(false);
  let rememberInput = $state<HTMLInputElement>();
  let denyButton = $state<HTMLButtonElement>();
  let allowButton = $state<HTMLButtonElement>();
  let busy = $derived(permissions.busy());
  let failure = $derived(permissions.failure());
  let camera = $derived(prompt.kinds.includes("camera"));
  let microphone = $derived(prompt.kinds.includes("microphone"));
  let capability = $derived(
    camera && microphone ? "camera and microphone" : camera ? "camera" : "microphone",
  );

  onMount(() => queueMicrotask(() => denyButton?.focus()));

  function respond(allow: boolean) {
    if (busy) return;
    const decision: PagePermissionPromptDecisionInput = allow
      ? remember
        ? "always_allow"
        : "allow_once"
      : remember
        ? "always_deny"
        : "deny_once";
    permissions.respond(prompt, decision);
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
    if (!prompt.rememberable) {
      if (event.shiftKey && document.activeElement === denyButton) {
        event.preventDefault();
        allowButton.focus();
      } else if (!event.shiftKey && document.activeElement === allowButton) {
        event.preventDefault();
        denyButton.focus();
      }
      return;
    }
    if (rememberInput === undefined) return;
    if (event.shiftKey && document.activeElement === rememberInput) {
      event.preventDefault();
      allowButton.focus();
    } else if (event.shiftKey && document.activeElement === denyButton) {
      event.preventDefault();
      rememberInput.focus();
    } else if (!event.shiftKey && document.activeElement === allowButton) {
      event.preventDefault();
      rememberInput.focus();
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<div class="fixed inset-0 z-50 flex items-center justify-center bg-canvas/80 p-4">
  <div
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="page-permission-title"
    aria-describedby="page-permission-origin page-permission-detail"
    class="w-[min(390px,calc(100vw-32px))] rounded-xl border border-border-strong bg-raised p-4 text-start shadow-[var(--shadow-overlay)]"
    data-zephium-page-permission-prompt
  >
    <div class="flex items-start gap-3">
      <span
        class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent"
        aria-hidden="true"
      >
        <Icon icon={camera ? Camera01Icon : Mic01Icon} size={19} />
      </span>
      <div class="min-w-0 flex-1">
        <h1 id="page-permission-title" class="text-[14px] leading-5 font-semibold text-text">
          Use your {capability}?
        </h1>
        <p id="page-permission-origin" class="mt-0.5 text-[11.5px] leading-4 break-all text-muted">
          {prompt.origin}
        </p>
      </div>
    </div>

    <p id="page-permission-detail" class="mt-3 text-[11.5px] leading-4 text-text">
      Granting access lets this site use your {capability}.
    </p>

    {#if prompt.rememberable}
      <label class="mt-3 flex cursor-pointer items-center gap-2 text-[11.5px] leading-4 text-muted">
        <input
          bind:this={rememberInput}
          type="checkbox"
          bind:checked={remember}
          disabled={busy}
          class="h-3.5 w-3.5 accent-accent"
        />
        Remember for this site
      </label>
    {:else}
      <p class="mt-3 rounded-md bg-fill px-2.5 py-2 text-[10.5px] leading-4 text-muted">
        Private Browsing won't remember this decision.
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
      <button
        bind:this={denyButton}
        type="button"
        disabled={busy}
        class="h-8 rounded-md border border-border-strong bg-fill px-3 text-[11.5px] font-medium text-text outline-none hover:bg-fill-hover focus-visible:shadow-[var(--shadow-focus)] disabled:cursor-default disabled:opacity-45"
        onclick={() => respond(false)}
      >
        Don't allow
      </button>
      <button
        bind:this={allowButton}
        type="button"
        disabled={busy}
        class="h-8 rounded-md bg-accent px-3 text-[11.5px] font-medium text-white outline-none hover:brightness-105 focus-visible:shadow-[var(--shadow-focus)] disabled:cursor-default disabled:opacity-45"
        onclick={() => respond(true)}
      >
        Allow
      </button>
    </div>
  </div>
</div>

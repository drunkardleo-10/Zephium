<script lang="ts">
  import { PuzzleIcon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import type { WebExtensionAccessRequestView } from "$shared/ipc/bindings";
  import { webext } from "$domain/webext";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import PromptSheet from "./PromptSheet.svelte";

  let { request }: { request: WebExtensionAccessRequestView } = $props();
  let denyButton = $state<HTMLButtonElement>();
  let busy = $derived(webext.isAnswering());
  let extension = $derived(webext.named(request.extension_id));
  let name = $derived(extension?.name ?? "An extension");

  onMount(() => queueMicrotask(() => denyButton?.focus()));

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !busy) {
      event.preventDefault();
      event.stopPropagation();
      void webext.answerAccess(false);
    }
  }
</script>

<svelte:window {onkeydown} />

<PromptSheet labelledby="web-extension-access-title" describedby="web-extension-access-list">
  <div class="flex items-start gap-3">
    <span
      class="flex h-10 w-10 shrink-0 items-center justify-center overflow-hidden rounded-control-compact bg-fill"
    >
      {#if extension?.icon}
        <img src={extension.icon} alt="" class="h-8 w-8" />
      {:else}
        <Icon icon={PuzzleIcon} size={20} />
      {/if}
    </span>
    <h1
      id="web-extension-access-title"
      class="min-w-0 flex-1 self-center text-[14px] leading-5 font-semibold text-text"
    >
      “{name}” wants more access
    </h1>
  </div>

  <div id="web-extension-access-list" class="mt-3 rounded-row bg-fill px-3 py-2.5">
    <p class="text-[10.5px] leading-4 font-medium tracking-wide text-faint uppercase">
      It would be able to
    </p>
    {#if request.warnings.length === 0}
      <p class="mt-1.5 text-[11.5px] leading-4 text-text">Use additional browser features.</p>
    {:else}
      <ul class="mt-1.5 space-y-1.5 text-[11.5px] leading-4 text-text">
        {#each request.warnings as warning (warning)}
          <li class="flex items-start gap-2">
            <span class="mt-[6px] h-1 w-1 shrink-0 rounded-full bg-accent"></span>
            <span>{warning}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="mt-4 flex justify-end gap-2">
    <Button
      bind:ref={denyButton}
      variant="secondary"
      disabled={busy}
      onclick={() => void webext.answerAccess(false)}
    >
      Deny
    </Button>
    <Button variant="primary" pending={busy} onclick={() => void webext.answerAccess(true)}>
      Allow
    </Button>
  </div>
</PromptSheet>

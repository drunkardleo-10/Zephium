<script lang="ts">
  import { PuzzleIcon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import type { WebExtensionReview } from "$shared/ipc/bindings";
  import { webext } from "$domain/webext";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";

  let { review }: { review: WebExtensionReview } = $props();
  let cancelButton = $state<HTMLButtonElement>();
  let busy = $derived(webext.isConfirming());
  let failure = $derived(webext.error());

  onMount(() => queueMicrotask(() => cancelButton?.focus()));

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !busy) {
      event.preventDefault();
      event.stopPropagation();
      webext.cancel();
    }
  }
</script>

<svelte:window {onkeydown} />

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-canvas/80 p-4"
  data-web-extension-review
>
  <div
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="web-extension-review-title"
    aria-describedby="web-extension-review-access"
    class="max-h-[min(560px,calc(100vh-32px))] w-[min(400px,calc(100vw-32px))] overflow-y-auto rounded-panel border border-border-strong bg-raised p-4 text-start shadow-[var(--shadow-overlay)]"
  >
    <div class="flex items-start gap-3">
      <span
        class="flex h-10 w-10 shrink-0 items-center justify-center overflow-hidden rounded-control-compact bg-fill"
      >
        {#if review.icon}
          <img src={review.icon} alt="" class="h-8 w-8" />
        {:else}
          <Icon icon={PuzzleIcon} size={20} />
        {/if}
      </span>
      <div class="min-w-0 flex-1">
        <h1 id="web-extension-review-title" class="text-[14px] leading-5 font-semibold text-text">
          {review.update ? "Update" : "Add"} “{review.name}”?
        </h1>
        <p class="mt-0.5 text-[11.5px] leading-4 text-muted">Version {review.version}</p>
      </div>
    </div>

    {#if review.description}
      <p class="mt-3 text-[11.5px] leading-4 text-muted">{review.description}</p>
    {/if}

    {#if review.from_file}
      <p class="mt-3 text-[11.5px] leading-4 text-warning">{m.webext_from_file()}</p>
    {/if}

    <div id="web-extension-review-access" class="mt-3 rounded-row bg-fill px-3 py-2.5">
      <p class="text-[10.5px] leading-4 font-medium tracking-wide text-faint uppercase">It can</p>
      {#if review.warnings.length === 0}
        <p class="mt-1.5 text-[11.5px] leading-4 text-text">Run without special access.</p>
      {:else}
        <ul class="mt-1.5 space-y-1.5 text-[11.5px] leading-4 text-text">
          {#each review.warnings as warning (warning)}
            <li class="flex items-start gap-2">
              <span class="mt-[6px] h-1 w-1 shrink-0 rounded-full bg-accent"></span>
              <span>{warning}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if failure !== null}
      <p role="alert" class="mt-2.5 text-[10.5px] leading-4 text-danger">{failure}</p>
    {/if}

    <div class="mt-4 flex justify-end gap-2">
      <Button
        bind:ref={cancelButton}
        variant="secondary"
        disabled={busy}
        onclick={() => webext.cancel()}
      >
        Cancel
      </Button>
      <Button variant="primary" pending={busy} onclick={() => void webext.confirm()}>
        {review.update ? "Update" : "Add extension"}
      </Button>
    </div>
  </div>
</div>

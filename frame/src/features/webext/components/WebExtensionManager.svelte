<script lang="ts">
  import { PuzzleIcon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import { webext } from "$domain/webext";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";

  let extensions = $derived(webext.list());
  let failure = $derived(webext.error());
  let removing = $state<string | null>(null);

  onMount(() => void webext.refresh());

  const stateLabel = (state: string) =>
    state === "running"
      ? "On"
      : state === "starting"
        ? "Starting…"
        : state === "failed"
          ? "Couldn't start"
          : "Off";
</script>

<section class="mx-auto w-full max-w-[680px] px-6 py-8" aria-labelledby="web-extensions-title">
  <h1 id="web-extensions-title" class="text-[20px] leading-7 font-semibold text-text">
    Extensions
  </h1>
  <p class="mt-1 text-[12.5px] leading-5 text-muted">
    Open an extension's page in the Chrome Web Store and choose Add to Zephium.
  </p>

  {#if failure !== null}
    <p role="alert" class="mt-4 text-[12px] leading-4 text-danger">{failure}</p>
  {/if}

  {#if extensions.length === 0}
    <div class="mt-8 flex flex-col items-center gap-2 text-center text-muted">
      <Icon icon={PuzzleIcon} size={28} />
      <p class="text-[12.5px]">No extensions yet.</p>
    </div>
  {:else}
    <ul class="mt-6 space-y-2">
      {#each extensions as extension (extension.id)}
        <li
          class="rounded-panel border border-border bg-raised p-3"
          data-web-extension={extension.id}
        >
          <div class="flex items-start gap-3">
            <span
              class="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-control-compact bg-fill"
            >
              {#if extension.icon}
                <img src={extension.icon} alt="" class="h-7 w-7" />
              {:else}
                <Icon icon={PuzzleIcon} size={18} />
              {/if}
            </span>
            <div class="min-w-0 flex-1">
              <div class="flex items-baseline gap-2">
                <h2 class="truncate text-[13.5px] leading-5 font-semibold text-text">
                  {extension.name}
                </h2>
                <span class="shrink-0 text-[11px] text-faint">{extension.version}</span>
              </div>
              <p
                class="text-[11.5px] leading-4"
                class:text-danger={extension.state === "failed"}
                class:text-muted={extension.state !== "failed"}
              >
                {stateLabel(extension.state)}{#if extension.error}: {extension.error}{/if}
              </p>
              {#if extension.description}
                <p class="mt-1 line-clamp-2 text-[11.5px] leading-4 text-muted">
                  {extension.description}
                </p>
              {/if}
            </div>
            <Button
              size="compact"
              variant={extension.enabled ? "secondary" : "primary"}
              aria-pressed={extension.enabled}
              onclick={() => void webext.setEnabled(extension.id, !extension.enabled)}
              >{extension.enabled ? "Turn off" : "Turn on"}</Button
            >
          </div>
          <div class="mt-2 flex justify-end">
            {#if removing === extension.id}
              <span class="me-2 self-center text-[11.5px] text-muted">Remove it and its data?</span>
              <Button size="compact" variant="secondary" onclick={() => (removing = null)}
                >Keep</Button
              >
              <Button
                size="compact"
                variant="danger"
                class="ms-1.5"
                onclick={() => {
                  removing = null;
                  void webext.uninstall(extension.id);
                }}>Remove</Button
              >
            {:else}
              <Button size="compact" variant="ghost" onclick={() => (removing = extension.id)}
                >Remove</Button
              >
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<script lang="ts">
  import { Popover } from "bits-ui";
  import { tick } from "svelte";
  import type { WorkModelEntry, WorkModelProvider } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import {
    ModelsSession,
    entryOf,
    keyState,
    pickerGroups,
    providerMark,
    providerName,
    shortName,
    usable,
  } from "$domain/ai";
  import Icon from "$shared/ui/Icon";
  import { ArrowDown01Icon, Key01Icon, Tick02Icon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";
  import "$shared/ui/Menu/popover.css";

  let {
    profile,
    onsettings = () => void commands.runCommand("settings.ai"),
  }: {
    profile: string;
    /** Opens Settings → AI, where keys are added. */
    onsettings?: () => void;
  } = $props();

  let session = $state.raw<ModelsSession | null>(null);
  $effect(() => {
    const owner = new ModelsSession(profile);
    session = owner;
    void owner.start();
    return () => owner.dispose();
  });

  let open = $state(false);
  let list = $state<HTMLElement>();

  const models = $derived(session?.models ?? null);
  const current = $derived(entryOf(models, models?.effective.lead));
  const chosen = $derived(models?.effective.lead ?? null);
  const groups = $derived(pickerGroups(models, "lead", session?.endpoint ?? []));
  const allKeyed = $derived(groups.needsKey.length === 0);
  /** No model can run yet: the one thing to do is add a key, so the control is that, not a menu. */
  const keyless = $derived(!!models && groups.ready.length === 0);

  async function opened(next: boolean) {
    open = next;
    if (!next) return;
    // The person's own server says which models it serves; the rest is the curated list.
    if (usable(models, "compatible")) void session?.listEndpoint();
    await tick();
    (list?.querySelector<HTMLElement>("[data-model-row][aria-current]") ?? rows()[0])?.focus();
  }

  /** The menu stays while the choice lands, so the mark moves before it closes. */
  async function choose(entry: WorkModelEntry) {
    if (entry.id !== chosen && !(await session?.choose("lead", entry.id))) return;
    open = false;
  }

  function settings() {
    open = false;
    onsettings();
  }

  function rows() {
    return [...(list?.querySelectorAll<HTMLElement>("[data-model-row]") ?? [])];
  }

  function move(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const all = rows();
    if (!all.length) return;
    event.preventDefault();
    const index = all.indexOf(document.activeElement as HTMLElement);
    if (index === -1) {
      all[event.key === "ArrowDown" ? 0 : all.length - 1]?.focus();
      return;
    }
    const next = index + (event.key === "ArrowDown" ? 1 : -1);
    all[Math.max(0, Math.min(next, all.length - 1))]?.focus();
  }
</script>

{#snippet model(entry: WorkModelEntry)}
  <li>
    <button
      type="button"
      class="ui-menu-item model-row"
      data-model-row
      aria-current={entry.id === chosen ? "true" : undefined}
      disabled={!!session?.busy}
      onclick={() => void choose(entry)}
    >
      <span class="name">{entry.display_name}</span>
      <span class="ui-menu-mark" aria-hidden="true"
        >{#if entry.id === chosen}<Icon icon={Tick02Icon} size={14} strokeWidth={2} />{/if}</span
      >
    </button>
  </li>
{/snippet}

{#snippet heading(provider: WorkModelProvider)}
  <div class="heading" role="presentation">
    <span class="heading-mark"
      ><Icon icon={providerMark(provider)} size={13} strokeWidth={1.6} /></span
    >
    <span>{providerName(provider)}</span>
  </div>
{/snippet}

{#if keyless}
  <button type="button" class="model-trigger add-key" onclick={settings}>
    <span class="trigger-mark" aria-hidden="true"><Icon icon={Key01Icon} size={14} /></span>
    <span class="trigger-name">{m.work_model_add_model_key()}</span>
  </button>
{:else}<Popover.Root {open} onOpenChange={(next) => void opened(next)}>
    <Popover.Trigger
      class="model-trigger"
      aria-label={current
        ? m.work_model_trigger({ model: current.display_name })
        : m.work_model_label()}
      disabled={!models}
    >
      {#if current}
        <span class="trigger-mark" aria-hidden="true"
          ><Icon icon={providerMark(current.model.provider)} size={14} strokeWidth={1.6} /></span
        >
        <span class="trigger-name">{shortName(current)}</span>
      {:else}
        <span class="trigger-name empty">{m.work_model_none()}</span>
      {/if}
      <span class="trigger-chevron" aria-hidden="true"
        ><Icon icon={ArrowDown01Icon} size={11} strokeWidth={2} /></span
      >
    </Popover.Trigger>
    <Popover.Portal>
      <Popover.Content
        class="ui-menu model-menu"
        side="top"
        align="end"
        sideOffset={10}
        collisionPadding={12}
        aria-label={m.work_model_label()}
        onkeydown={move}
      >
        <div class="pane" bind:this={list}>
          {#each groups.ready as group (group.provider)}
            <section class="group" aria-label={providerName(group.provider)}>
              {@render heading(group.provider)}
              <ul>
                {#each group.entries as entry (entry.id)}{@render model(entry)}{/each}
              </ul>
            </section>
          {/each}
          {#if groups.needsKey.length}
            {#if groups.ready.length}<div class="ui-menu-separator" role="presentation"></div>{/if}
            <ul aria-label={m.work_model_needs_key()}>
              {#each groups.needsKey as provider (provider)}
                <li>
                  <button
                    type="button"
                    class="ui-menu-item keyless"
                    data-model-row
                    onclick={settings}
                  >
                    <span class="ui-menu-icon" aria-hidden="true"
                      ><Icon icon={providerMark(provider)} size={15} strokeWidth={1.6} /></span
                    >
                    <span class="name">{providerName(provider)}</span>
                    <span class="hint"
                      >{keyState(models, provider) === "invalid"
                        ? m.work_model_key_refused()
                        : m.work_model_needs_key()}</span
                    >
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
          <div class="ui-menu-separator" role="presentation"></div>
          <button type="button" class="ui-menu-item" data-model-row onclick={settings}>
            <span class="ui-menu-icon" aria-hidden="true"><Icon icon={Key01Icon} size={15} /></span>
            <span class="name">{allKeyed ? m.work_model_manage() : m.work_model_add_key()}</span>
          </button>
        </div>
        {#if session?.fault?.action === "choose:lead"}<p class="note failure" role="alert">
            {m.work_model_failed()}
          </p>{/if}
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>{/if}

<style>
  /* Calm, not an alarm: the model's place, asking for the key it needs. */
  .add-key {
    color: var(--color-label-secondary);
  }

  :global(.model-trigger) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-inline-size: 180px;
    block-size: 28px;
    margin-block: 3px;
    padding-inline: 8px 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  :global(.model-trigger:hover:not(:disabled)),
  :global(.model-trigger[data-state="open"]) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  :global(.model-trigger:focus-visible) {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  :global(.model-trigger:disabled) {
    opacity: 0;
  }

  .trigger-mark,
  .trigger-chevron {
    display: grid;
    flex: none;
    place-items: center;
  }

  .trigger-chevron {
    color: var(--color-faint);
  }

  .trigger-name {
    overflow: hidden;
    font-weight: 500;
    letter-spacing: -0.003em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .trigger-name.empty {
    font-weight: 450;
  }

  /* Over the canvas the menu stands on the solid floating material, like
     every Work menu. */
  :global(.model-menu) {
    display: flex;
    flex-direction: column;
    inline-size: 272px;
    max-block-size: min(460px, var(--bits-floating-available-height, 460px));
    background: var(--color-float);
    backdrop-filter: none;
    transform-origin: var(--bits-floating-transform-origin, bottom right);
  }

  .pane {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-block-size: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .group + .group {
    margin-block-start: 4px;
  }

  /* Headings, model names and keyless providers share one text column:
     the mark sits in the gutter the menu's icons use. */
  .heading {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px var(--menu-item-inset) 4px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 550;
    line-height: 14px;
    letter-spacing: 0.01em;
  }

  .heading-mark {
    display: grid;
    flex: none;
    inline-size: 16px;
    place-items: center;
  }

  .model-row {
    padding-inline-start: calc(var(--menu-item-inset) + 26px);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  button.ui-menu-item {
    inline-size: 100%;
    border: 0;
    background: transparent;
    font: inherit;
    text-align: start;
  }

  button.ui-menu-item:disabled {
    opacity: 0.45;
  }

  button.ui-menu-item:focus-visible,
  button.ui-menu-item:hover:not(:disabled) {
    background: var(--row-active);
  }

  .name {
    flex: 1;
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .keyless .name {
    color: var(--color-muted);
  }

  .hint {
    flex: none;
    color: var(--color-faint);
    font-size: var(--text-label);
  }

  .keyless:hover .hint,
  .keyless:focus-visible .hint {
    color: var(--color-muted);
  }

  .note {
    margin: 0;
    padding: 10px var(--menu-item-inset);
    color: var(--color-faint);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .failure {
    color: var(--color-danger);
  }
</style>

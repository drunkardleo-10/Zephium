<script lang="ts">
  import { tabs } from "$domain/tabs";
  import { blockerSites } from "$domain/blocker";
  import type {
    BlockerSiteContext,
    BlockerPickerView,
    BlockerPickerAction,
  } from "$shared/ipc/bindings";
  import { onDestroy } from "svelte";

  let { context, disabled = false }: { context: BlockerSiteContext; disabled?: boolean } = $props();
  let picker = $state<BlockerPickerView | null>(null);
  let busy = $state(false);
  let feedback = $state("");
  let previewing = $state(false);
  let lifetime = 0;
  let owner: BlockerSiteContext | null = null;
  let key = $derived(`${context.profile}:${context.tab}:${context.site}:${context.revision}`);
  function cancel() {
    lifetime += 1;
    const previous = picker;
    const previousOwner = owner;
    picker = null;
    owner = null;
    previewing = false;
    busy = false;
    if (previous && previousOwner)
      void blockerSites.picker(previousOwner, { kind: "stop", session: previous.session });
  }
  $effect(() => {
    void key;
    void tabs.activeId();
    void tabs.activeTab()?.url;
    void tabs.activeTab()?.loading;
    return cancel;
  });
  onDestroy(cancel);

  async function request(action: BlockerPickerAction) {
    if (busy) return;
    const generation = lifetime;
    const target = owner ?? { ...context };
    busy = true;
    feedback = "";
    const result = await blockerSites.picker(target, action);
    if (generation !== lifetime) {
      if (result?.active)
        void blockerSites.picker(target, { kind: "stop", session: result.session });
      return;
    }
    busy = false;
    if (!result) {
      feedback = "The page changed or the picker is unavailable. Try again.";
      return;
    }
    owner = target;
    picker = result.active ? result : null;
    previewing = action.kind === "preview" && action.enabled && result.selection !== null;
    if (!result.active) feedback = "Selection ended.";
    else if (action.kind !== "start" && !result.selection)
      feedback = "Select an element on the page first.";
  }

  async function save() {
    if (!picker?.selection || !owner || busy) return;
    const generation = lifetime;
    const target = owner;
    const selected = picker;
    busy = true;
    const result = await blockerSites.changeSite(target, {
      kind: "save_selection",
      session: selected.session,
      selection: selected.selection!.identity,
    });
    if (generation !== lifetime) return;
    busy = false;
    if (result.state === "processed" && result.disposition.outcome === "applied") {
      cancel();
      feedback = "Hide saved. You can undo it under Hidden elements.";
    } else {
      feedback = "Could not save this selection. The page may have changed. Select it again.";
    }
  }
</script>

<div class="picker" data-keep-open>
  {#if !picker}
    <button
      class="ui-menu-item action"
      type="button"
      role="menuitem"
      disabled={disabled || busy}
      onclick={() => void request({ kind: "start" })}
      >{busy ? "Opening picker…" : "Hide an element…"}</button
    >
  {:else}
    <p class="hint">
      Click an element on the page, then reopen this menu to preview it. Escape cancels.
    </p>
    <button
      class="ui-menu-item action"
      type="button"
      role="menuitem"
      disabled={busy}
      onclick={() => {
        if (picker)
          void request({ kind: "preview", session: picker.session, enabled: !previewing });
      }}>{previewing ? "Undo preview" : "Preview selection"}</button
    >
    {#if picker.selection}
      <p class="hint">
        {picker.selection.label} · {picker.selection.count} matching {picker.selection.count === 1
          ? "element"
          : "elements"}
      </p>
      {#if picker.selection.positional}<p class="hint">
          This selection follows the page layout and may change when the site is redesigned.
        </p>{/if}
      <button
        class="ui-menu-item action"
        type="button"
        role="menuitem"
        disabled={busy || !previewing}
        onclick={() => void save()}>Save hide</button
      >
    {/if}
    <button
      class="ui-menu-item action"
      type="button"
      role="menuitem"
      disabled={busy}
      onclick={cancel}>Cancel</button
    >
  {/if}
  {#if feedback}<p class="hint" role="status">{feedback}</p>{/if}
</div>

<style>
  .picker {
    min-inline-size: 0;
  }

  .action {
    inline-size: 100%;
    border: 0;
    background: transparent;
    text-align: start;
    font: inherit;
    font-size: var(--text-body);
  }

  .action:hover,
  .action:focus-visible {
    background: var(--row-active);
  }

  .action:disabled {
    opacity: 0.5;
  }

  .hint {
    max-inline-size: 220px;
    margin: 4px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }
</style>

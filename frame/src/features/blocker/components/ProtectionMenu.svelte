<script lang="ts">
  import { Shield01Icon } from "@hugeicons/core-free-icons";
  import { blocker, blockerSites, shieldPresentation } from "$domain/blocker";
  import { commands } from "$shared/ipc/bindings";
  import type { BlockerSiteAction } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import ElementPickerControls from "./ElementPickerControls.svelte";

  let { labelled = false }: { labelled?: boolean } = $props();
  let status = $derived(blocker.status());
  let shield = $derived(shieldPresentation(status));
  let site = $derived(status.site);
  let saving = $state(false);
  let managing = $state(false);
  let feedback = $state("");
  let reloadSuggested = $state(false);
  let contextKey = $derived(
    site ? `${site.context.profile}:${site.context.tab}:${site.context.site}` : "",
  );
  $effect(() => {
    void contextKey;
    feedback = "";
    reloadSuggested = false;
    managing = false;
  });

  async function enable() {
    if (saving) return;
    saving = true;
    try {
      const result = await blocker.setEnabled(true);
      feedback =
        result.state === "processed" && ["applied", "no_op"].includes(result.disposition.outcome)
          ? "Protection enabled."
          : result.state === "pending"
            ? "Protection starting… You can keep browsing."
            : "Could not enable protection. Try again from Privacy settings.";
    } finally {
      saving = false;
    }
  }

  async function change(action: BlockerSiteAction) {
    if (!site || saving) return;
    const context = site.context;
    const key = contextKey;
    saving = true;
    feedback = "";
    try {
      const result = await blockerSites.changeSite(context, action);
      if (key !== contextKey) return;
      if (
        result.state === "processed" &&
        (result.disposition.outcome === "applied" || result.disposition.outcome === "no_op")
      ) {
        feedback =
          action.kind === "pause" ? "Saved. Reload to apply to requests already made." : "Saved.";
        reloadSuggested = action.kind === "pause";
      } else {
        feedback = "Could not confirm this change. Check the current setting or try again.";
      }
    } finally {
      saving = false;
    }
  }
</script>

{#if shield.visible && labelled}
  <div class="protection" data-keep-open>
    <div class="standing" class:warning={shield.tone === "warning"} role="status">
      <Icon icon={Shield01Icon} size={15} />
      <span
        >{site?.paused && status.applied_enabled === true
          ? "Paused on this site"
          : shield.label}</span
      >
    </div>
    {#if status.desired_enabled !== true}
      <button
        type="button"
        role="menuitem"
        class="ui-menu-item action"
        disabled={saving || !status.can_enable || status.preference !== "authoritative"}
        onclick={() => void enable()}>Enable protection</button
      >
    {/if}
    {#if site}
      <p class="site" title={site.context.site}>{site.context.site}</p>
      {#if site.private_session}<p class="hint">For this private session</p>{/if}
      {#if site.ready}
        <button
          class="ui-menu-item action"
          type="button"
          role="menuitem"
          disabled={saving || site.busy || status.applied_enabled !== true}
          onclick={() => void change({ kind: "pause", paused: !site.paused })}
        >
          {saving || site.busy
            ? "Saving…"
            : site.paused
              ? "Resume on this site"
              : "Pause on this site"}
        </button>
      {:else}
        <button
          class="ui-menu-item action"
          type="button"
          role="menuitem"
          disabled={saving || site.busy}
          onclick={() => void change({ kind: "retry" })}>Retry site controls</button
        >
      {/if}
      <ElementPickerControls context={site.context} disabled={!site.ready || site.busy || saving} />
      {#if site.hides.length > 0}
        <button
          class="ui-menu-item action"
          type="button"
          role="menuitem"
          aria-expanded={managing}
          onclick={() => (managing = !managing)}>Hidden elements · {site.hides.length}</button
        >
        {#if managing}
          <div class="hides">
            {#each site.hides as hide (hide.id)}
              <div class="hide-row">
                <span class="hide-label" title={hide.label}>{hide.label}</span>
                <button
                  type="button"
                  role="menuitem"
                  disabled={saving || site.busy}
                  onclick={() =>
                    void change({ kind: "set_hide_enabled", id: hide.id, enabled: !hide.enabled })}
                  >{hide.enabled ? "Show" : "Hide"}</button
                >
                <button
                  type="button"
                  role="menuitem"
                  aria-label={`Remove ${hide.label}`}
                  disabled={saving || site.busy}
                  onclick={() => void change({ kind: "remove_hide", id: hide.id })}>Remove</button
                >
              </div>
            {/each}
          </div>
        {/if}
      {/if}
      {#if reloadSuggested}<button
          class="ui-menu-item action"
          type="button"
          role="menuitem"
          onclick={() => {
            if (site) void commands.tabsReload(site.context.tab);
          }}>Reload page</button
        >{/if}
    {:else}
      <p class="hint">Site controls are available on web pages.</p>
    {/if}
    {#if feedback}<p class="hint" role="status">{feedback}</p>{/if}
  </div>
{:else if shield.visible}
  <span
    class="compact"
    class:warning={shield.tone === "warning"}
    title={shield.label}
    role="img"
    aria-label={shield.label}><Icon icon={Shield01Icon} size={14} /></span
  >
{/if}

<style>
  .protection {
    min-inline-size: 0;
    padding: 4px;
  }

  .standing {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px;
    color: var(--color-text);
    font-size: var(--text-body);
  }

  .warning {
    color: var(--color-warning);
  }

  .site {
    overflow: hidden;
    max-inline-size: 220px;
    padding: 0 4px;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-muted);
    font-size: var(--text-body);
  }

  .hint {
    max-inline-size: 220px;
    margin: 4px;
    color: var(--color-muted);
    font-size: var(--text-caption);
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

  button:disabled {
    opacity: 0.5;
  }

  .hides {
    max-block-size: 180px;
    overflow: auto;
  }

  .hide-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
  }

  .hide-label {
    flex: 1;
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-caption);
  }

  .hide-row button {
    border: 0;
    border-radius: var(--radius-inset);
    padding: 4px;
    background: var(--row-hover);
    color: var(--color-text);
    font-size: var(--text-caption);
  }

  .compact {
    display: grid;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
    color: var(--color-faint);
  }
</style>

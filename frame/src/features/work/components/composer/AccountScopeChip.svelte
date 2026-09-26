<script lang="ts">
  import type { WorkAccountEffectV1, WorkAccountModeV1 } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import { BrowserIcon, Cancel01Icon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    title,
    origin,
    mode = "page",
    effect = $bindable({ kind: "read" }),
    disabled = false,
    onremove,
  }: {
    title: string;
    origin: string;
    /** `origin`: the agent may read pages across the tab's site for one request, reading only. */
    mode?: WorkAccountModeV1;
    effect?: WorkAccountEffectV1;
    disabled?: boolean;
    onremove: () => void;
  } = $props();
  const id = $props.id();
  const update = $derived(effect.kind === "update" ? effect.update : null);
  const host = $derived.by(() => {
    try {
      return new URL(origin).host;
    } catch {
      return origin;
    }
  });
  function setUpdate(patch: Partial<{ field: string | null; from: string; to: string }>) {
    const current = update ?? { field: null, from: "", to: "" };
    effect = { kind: "update", update: { ...current, ...patch } };
  }
</script>

<div class="account-scope" role="group" aria-label={m.work_account_scope()}>
  <span class="chip">
    <Icon icon={BrowserIcon} size={12} />
    <span class="title">{title}</span>
    <span class="origin">{origin}</span>
    <button
      type="button"
      class="remove"
      aria-label={m.work_account_scope_remove()}
      {disabled}
      onclick={onremove}><Icon icon={Cancel01Icon} size={12} /></button
    >
  </span>
  {#if mode === "origin"}<p class="note">{m.work_account_read_origin({ host })}</p>
  {:else}<div class="effect">
      <label
        ><input
          type="radio"
          name={`${id}-effect`}
          checked={effect.kind === "read"}
          {disabled}
          onchange={() => (effect = { kind: "read" })}
        />{m.work_account_effect_read()}</label
      >
      <label
        ><input
          type="radio"
          name={`${id}-effect`}
          checked={effect.kind === "update"}
          {disabled}
          onchange={() => setUpdate({})}
        />{m.work_account_effect_update()}</label
      >
    </div>
    {#if update}
      <div class="fields">
        <label
          >{m.work_account_field()}<input
            type="text"
            maxlength="128"
            value={update.field ?? ""}
            placeholder={m.work_account_field_placeholder()}
            {disabled}
            oninput={(event) => setUpdate({ field: event.currentTarget.value || null })}
          /></label
        >
        <label
          >{m.work_account_from()}<input
            type="text"
            maxlength="512"
            value={update.from}
            {disabled}
            oninput={(event) => setUpdate({ from: event.currentTarget.value })}
          /></label
        >
        <label
          >{m.work_account_to()}<input
            type="text"
            maxlength="512"
            value={update.to}
            {disabled}
            oninput={(event) => setUpdate({ to: event.currentTarget.value })}
          /></label
        >
      </div>
      <p class="note">{m.work_account_update_note()}</p>
    {/if}
    <p class="note">{m.work_account_disclosure()}</p>{/if}
</div>

<style>
  .account-scope {
    display: flex;
    flex-direction: column;
    gap: 6px;
    inline-size: 100%;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-inline-size: 100%;
    padding: 2px 4px 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-active);
    color: var(--color-text);
    font-size: var(--text-caption);
    justify-self: start;
  }

  .title,
  .origin {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .origin {
    color: var(--color-muted);
  }

  .remove {
    display: grid;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--color-muted);
    cursor: default;
  }

  .remove:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .effect {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  .effect label,
  .fields label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: 8px;
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  .fields label {
    flex-direction: column;
    align-items: stretch;
    gap: 2px;
  }

  .fields input {
    box-sizing: border-box;
    inline-size: 100%;
    padding: 4px 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
  }

  .fields input:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .note {
    margin: 0;
    color: var(--color-muted);
    font-size: 11.5px;
  }
</style>

<script lang="ts">
  import { tick } from "svelte";
  import * as m from "$shared/i18n/messages";
  import type { WorkDecisionChoiceV1 } from "$shared/ipc/bindings";
  import { WorkDecisionSession } from "$domain/work-decision";
  import SegmentedControl from "$shared/ui/SegmentedControl";
  import Button from "$shared/ui/Button";
  import Field from "$shared/ui/Field";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import {
    decisionActive,
    decisionChoices,
    decisionDetail,
    decisionDisclosure,
    decisionLabel,
  } from "../lib/work-decisions";
  let { profile }: { profile: string } = $props();
  let session = $state.raw<WorkDecisionSession | null>(null);
  let editing = $state(false);
  let secret = $state("");
  let editor = $state<HTMLFormElement>();
  $effect(() => {
    const owner = new WorkDecisionSession(profile);
    session = owner;
    void owner.start();
    return () => {
      owner.dispose();
      secret = "";
      editing = false;
    };
  });
  let preference = $derived(session?.preference ?? null);
  let choice = $derived<WorkDecisionChoiceV1>(preference?.choice ?? "recommended");
  let effective = $derived<WorkDecisionChoiceV1>(preference?.effective ?? "standard");
  let active = $derived(preference ? decisionActive(choice, effective) : null);
  let options = $derived(decisionChoices.map((value) => ({ value, label: decisionLabel(value) })));
  let keyFailure = $derived(session?.keyFailure);
  let keyMessage = $derived(
    keyFailure === "invalid"
      ? m.ai_jev_key_invalid()
      : keyFailure === "unconfirmed" || keyFailure === "outcome_unknown"
        ? m.ai_jev_key_unconfirmed()
        : keyFailure
          ? m.ai_jev_key_failed()
          : null,
  );

  async function save() {
    const submitted = secret;
    secret = "";
    if (await session?.setKey(submitted)) editing = false;
  }

  async function edit() {
    secret = "";
    editing = true;
    await tick();
    editor?.querySelector("input")?.focus();
  }

  function cancel() {
    secret = "";
    editing = false;
  }
</script>

<SettingsGroup title={m.ai_jev_group()}>
  <SettingsRow
    settingId="work.decisions"
    title={m.settings_decisions()}
    description={m.settings_decisions_desc()}
  >
    <div class="decisions">
      <SegmentedControl
        label={m.settings_decisions()}
        {options}
        value={choice}
        disabled={!preference || session?.busy}
        onchange={(value) => void session?.choose(value as WorkDecisionChoiceV1)}
      />
      {#if active}<p class="decisions-active" role="status">{active}</p>{/if}
    </div>
  </SettingsRow>
  <SettingsRow
    settingId="work.decisions.key"
    title={m.ai_jev_key_title()}
    description={m.ai_jev_key_desc()}
  >
    <div class="decisions">
      <span class="decisions-active" role="status"
        >{preference?.typesafe_key_present ? m.ai_key_set() : m.ai_key_missing()}</span
      >
      <div class="key-actions">
        <Button size="compact" disabled={!preference || session?.busy} onclick={() => void edit()}
          >{preference?.typesafe_key_present ? m.ai_key_replace() : m.ai_jev_key_add()}</Button
        >
        {#if preference?.typesafe_key_present}<Button
            size="compact"
            variant="ghost"
            disabled={session?.busy}
            onclick={() => {
              secret = "";
              editing = false;
              void session?.clearKey();
            }}>{m.ai_key_remove()}</Button
          >{/if}
      </div>
    </div>
  </SettingsRow>
  {#if editing}<form
      class="key-editor"
      bind:this={editor}
      onsubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <Field
        label={m.ai_jev_key_title()}
        type="password"
        autocomplete="off"
        spellcheck="false"
        bind:value={secret}
        onkeydown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            cancel();
          }
        }}
      />
      <div class="key-actions">
        <Button size="compact" variant="ghost" onclick={cancel}>{m.ai_key_cancel()}</Button><Button
          size="compact"
          type="submit"
          variant="primary"
          pending={session?.busy}
          disabled={!secret.trim() || session?.busy}>{m.ai_key_save()}</Button
        >
      </div>
    </form>{/if}
  {#if keyMessage}<p class="key-message" role="alert">{keyMessage}</p>{/if}
</SettingsGroup>
{#if session?.unavailable}<p class="decisions-note" role="alert">
    {m.settings_decisions_unavailable()}
  </p>{:else if preference}<p class="decisions-note">
    {decisionDetail(choice)}
    {decisionDisclosure(effective)}
  </p>{/if}

<style>
  .decisions {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 6px;
    min-width: 0;
  }

  .decisions-active {
    margin: 0;
    font-size: var(--text-label);
    line-height: 1.5;
    color: var(--color-muted);
  }

  .decisions-note {
    margin: -20px 16px 28px;
    font-size: var(--text-label);
    line-height: 1.5;
    color: var(--color-muted);
  }

  .key-editor {
    display: grid;
    gap: 12px;
    padding: 12px 18px 16px;
  }

  .key-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .key-message {
    margin: 0;
    padding: 0 18px 16px;
    color: var(--color-danger);
    font-size: var(--text-label);
    line-height: 1.5;
  }
</style>

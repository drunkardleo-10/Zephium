<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { WorkDecisionChoiceV1 } from "$shared/ipc/bindings";
  import { WorkDecisionSession } from "$domain/work-decision";
  import SegmentedControl from "$shared/ui/SegmentedControl";
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
  $effect(() => {
    const owner = new WorkDecisionSession(profile);
    session = owner;
    void owner.start();
    return () => owner.dispose();
  });
  let preference = $derived(session?.preference ?? null);
  let choice = $derived<WorkDecisionChoiceV1>(preference?.choice ?? "recommended");
  let effective = $derived<WorkDecisionChoiceV1>(preference?.effective ?? "standard");
  let active = $derived(preference ? decisionActive(choice, effective) : null);
  let options = $derived(decisionChoices.map((value) => ({ value, label: decisionLabel(value) })));
</script>

<SettingsGroup title={m.section_work()}>
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
</style>

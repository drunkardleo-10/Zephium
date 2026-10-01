<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { tabs } from "$domain/tabs";
  import { preferences } from "$domain/preferences";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Switch from "$shared/ui/Switch";
  import WorkDecisions from "../WorkDecisions.svelte";
  let profile = $derived(tabs.profile());
</script>

<SettingsGroup title={m.settings_work_availability()}>
  <SettingsRow
    settingId="work.enabled"
    title={m.settings_work_enabled()}
    description={m.settings_work_enabled_desc()}
    ><Switch
      label={m.settings_work_enabled()}
      labelHidden
      checked={preferences.value("work.enabled") !== "false"}
      disabled={preferences.saving()}
      onchange={(on) => void preferences.set("work.enabled", String(on))}
    /></SettingsRow
  >
  <SettingsRow
    settingId="ai.enabled"
    title={m.settings_work_ai()}
    description={m.settings_work_ai_desc()}
    ><Switch
      label={m.settings_work_ai()}
      labelHidden
      checked={preferences.value("ai.enabled") !== "false"}
      disabled={preferences.saving()}
      onchange={(on) => void preferences.set("ai.enabled", String(on))}
    /></SettingsRow
  >
</SettingsGroup>
{#if preferences.saveFailed()}<p class="work-note" role="status">{m.settings_work_failed()}</p>{/if}
{#if profile && profile.kind !== "incognito"}<WorkDecisions profile={profile.id} />{:else}<p
    class="work-note"
  >
    {m.work_regular_profile()}
  </p>{/if}

<style>
  .work-note {
    margin: 0 16px;
    font-size: var(--text-label);
    line-height: 1.5;
    color: var(--color-muted);
  }
</style>

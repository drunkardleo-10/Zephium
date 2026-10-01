<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import Select from "$shared/ui/Select";
  import Switch from "$shared/ui/Switch";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";

  let greeting = $derived(preferences.value("ui.newtab-greeting") === "true");
  let clock = $derived(preferences.value("ui.newtab-clock") === "true");

  const set = (key: preferences.PreferenceKey, value: string) => void preferences.set(key, value);
</script>

<SettingsGroup title={m.settings_newtab_content()}>
  <SettingsRow
    settingId="ntp.greeting"
    title={m.pref_ntp_greeting()}
    description={m.pref_ntp_greeting_help()}
    ><Switch
      label={m.pref_ntp_greeting()}
      labelHidden
      checked={greeting}
      disabled={preferences.saving()}
      onchange={(v) => set("ui.newtab-greeting", String(v))}
    /></SettingsRow
  >
  <SettingsRow
    settingId="ntp.personalize"
    title={m.pref_ntp_personalize()}
    description={m.pref_ntp_personalize_help()}
    ><Switch
      label={m.pref_ntp_personalize()}
      labelHidden
      checked={preferences.value("ui.newtab-name") === "true"}
      disabled={preferences.saving() || !greeting}
      onchange={(v) => set("ui.newtab-name", String(v))}
    /></SettingsRow
  >
  <SettingsRow
    settingId="ntp.clock"
    title={m.pref_ntp_clock()}
    description={m.pref_ntp_clock_help()}
    ><Switch
      label={m.pref_ntp_clock()}
      labelHidden
      checked={clock}
      disabled={preferences.saving()}
      onchange={(v) => set("ui.newtab-clock", String(v))}
    /></SettingsRow
  >
  <SettingsRow
    settingId="ntp.clock-format"
    title={m.pref_ntp_clock_format()}
    description={m.pref_ntp_clock_format_help()}
    ><Select
      label={m.pref_ntp_clock_format()}
      labelHidden
      value={preferences.value("ui.newtab-clock-format")}
      disabled={preferences.saving() || !clock}
      options={[
        { value: "system", label: m.pref_ntp_clock_format_option_0() },
        { value: "12h", label: m.pref_ntp_clock_format_option_1() },
        { value: "24h", label: m.pref_ntp_clock_format_option_2() },
      ]}
      onchange={(v) => set("ui.newtab-clock-format", v)}
    /></SettingsRow
  >
  <SettingsRow
    settingId="ntp.tasks"
    title={m.pref_ntp_tasks()}
    description={m.pref_ntp_tasks_help()}
    ><Switch
      label={m.pref_ntp_tasks()}
      labelHidden
      checked={preferences.value("ui.newtab-tasks") === "true"}
      disabled={preferences.saving()}
      onchange={(v) => set("ui.newtab-tasks", String(v))}
    /></SettingsRow
  >
</SettingsGroup>

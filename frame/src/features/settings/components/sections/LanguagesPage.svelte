<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import { languages } from "$shared/lib/locale.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Select from "$shared/ui/Select";

  /** Each language named in itself, the way people look for their own. */
  function name(code: string) {
    try {
      return new Intl.DisplayNames([code], { type: "language" }).of(code) ?? code;
    } catch {
      return code;
    }
  }

  let options = $derived([
    { value: "system", label: m.language_system() },
    ...languages()
      .map((code) => ({ value: code, label: name(code) }))
      .sort((a, b) => a.label.localeCompare(b.label)),
  ]);
</script>

<SettingsGroup title={m.language_interface_title()} description={m.language_websites_note()}>
  <SettingsRow
    settingId="languages.interface"
    title={m.language_interface_title()}
    description={m.language_interface_help()}
    ><Select
      label={m.language_interface_title()}
      labelHidden
      value={preferences.value("ui.language")}
      disabled={preferences.saving()}
      {options}
      onchange={(value) => void preferences.set("ui.language", value)}
    /></SettingsRow
  >
</SettingsGroup>

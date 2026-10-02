<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreferenceSelect from "../PreferenceSelect.svelte";
  import PreferenceSwitch from "../PreferenceSwitch.svelte";
</script>

<SettingsGroup title={m.section_tabs()}
  ><PreferenceSelect id="tabs.new-position" preference="tabs.new-position" /><PreferenceSelect
    id="tabs.close"
    preference="tabs.after-close"
  /><PreferenceSwitch id="tabs.duplicates" preference="tabs.switch-to-open" /></SettingsGroup
>
<PreviewNotice />
<SettingsGroup title={m.settings_spaces()}><PreviewSelect id="tabs.archive" /></SettingsGroup>

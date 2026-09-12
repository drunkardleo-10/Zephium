<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewSlider from "../PreviewSlider.svelte";
</script>

<PreviewNotice />
<SettingsGroup title={m.section_tabs()}
  ><PreviewSelect id="tabs.new-position" /><PreviewSelect id="tabs.close" /><PreviewToggle
    id="tabs.duplicates"
  /></SettingsGroup
>
<SettingsGroup title={m.settings_spaces()}
  ><PreviewToggle id="tabs.folders" /><PreviewToggle id="tabs.pins" /><PreviewSelect
    id="tabs.archive"
  /><PreviewSlider
    id="tabs.sleep"
    label={m.pref_tabs_sleep_short()}
    format={(value) => `${value} min`}
  /><PreviewToggle id="tabs.splits" /></SettingsGroup
>

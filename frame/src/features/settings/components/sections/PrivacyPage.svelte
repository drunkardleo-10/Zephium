<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import * as preview from "../../lib/preview.svelte";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewAction from "../PreviewAction.svelte";
  import SiteExceptions from "../SiteExceptions.svelte";
  import Checkbox from "$shared/ui/Checkbox";
  import Select from "$shared/ui/Select";
</script>

<PreviewNotice />
<SettingsGroup title={m.settings_protection()}
  ><PreviewToggle id="privacy.blocking" /><PreviewSelect id="privacy.cookies" /><PreviewToggle
    id="privacy.https"
  /></SettingsGroup
>
<SettingsGroup title={m.settings_site_permissions()}
  ><PreviewSelect id="privacy.camera" /><PreviewSelect id="privacy.microphone" /><PreviewSelect
    id="privacy.location"
  /><PreviewSelect id="privacy.notifications" /></SettingsGroup
>
<SettingsGroup title={m.privacy_media_group()}
  ><PreviewSelect id="privacy.popups" /><PreviewSelect id="privacy.clipboard" /><PreviewSelect
    id="privacy.automatic-downloads"
  /><PreviewSelect id="privacy.sound" /></SettingsGroup
>
<SiteExceptions />
<SettingsGroup title={m.settings_browsing_data()}
  ><PreviewToggle id="privacy.cleanup" /><PreviewAction
    id="privacy.clear"
    valid={["history", "cookies", "cache"].some((key) => preview.get(`clear.${key}`, false))}
    ><Select
      label={m.preview_time_range()}
      value={preview.get("clear.range", "hour")}
      options={[
        { value: "hour", label: m.preview_range_hour() },
        { value: "day", label: m.preview_range_day() },
        { value: "all", label: m.preview_range_all() },
      ]}
      onchange={(value) => preview.set("clear.range", value)}
    />{#each [{ key: "history", label: m.preview_history }, { key: "cookies", label: m.preview_cookies }, { key: "cache", label: m.preview_cache }] as item (item.key)}<Checkbox
        label={item.label()}
        checked={preview.get(`clear.${item.key}`, false)}
        onchange={(value) => preview.set(`clear.${item.key}`, value)}
      />{/each}</PreviewAction
  ></SettingsGroup
>

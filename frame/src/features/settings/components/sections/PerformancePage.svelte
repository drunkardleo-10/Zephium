<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import CollectionEditor from "../CollectionEditor.svelte";
</script>

<PreviewNotice />
<SettingsGroup title={m.settings_sleeping_tabs()}
  ><PreviewToggle id="performance.sleep" /><PreviewSelect id="performance.after" /></SettingsGroup
><SettingsGroup title={m.settings_resource_policy()}
  ><PreviewSelect id="performance.memory" /></SettingsGroup
><CollectionEditor id="performance.exceptions" kind="url" />

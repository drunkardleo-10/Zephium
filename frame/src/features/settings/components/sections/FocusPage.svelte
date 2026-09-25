<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import CollectionEditor from "../CollectionEditor.svelte";
</script>

<PreviewNotice />
<SettingsGroup title={m.section_focus()}
  ><PreviewToggle id="focus.track" /><PreviewSelect id="focus.retention" /></SettingsGroup
><CollectionEditor id="focus.exclusions" kind="url" /><CollectionEditor id="focus.rules" />

<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import CollectionEditor from "../CollectionEditor.svelte";
</script>

<PreviewNotice />
<SettingsGroup title={m.section_search()}
  ><PreviewSelect id="search.provider" /><PreviewToggle id="search.suggestions" /><PreviewToggle
    id="search.history"
  /><PreviewToggle id="search.open-new" /></SettingsGroup
>
<CollectionEditor id="search.engines" kind="url" /><CollectionEditor
  id="search.shortcuts"
  kind="url"
/>

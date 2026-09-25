<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewToggle from "../PreviewToggle.svelte";
  import CollectionEditor from "../CollectionEditor.svelte";
</script>

<PreviewNotice />
<SettingsGroup title={m.section_passwords()}
  ><PreviewToggle id="passwords.offer" /><PreviewToggle id="passwords.fill" /></SettingsGroup
><CollectionEditor id="passwords.entries" kind="credential" /><CollectionEditor
  id="passwords.addresses"
/>

<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { showPreviews } from "../../lib/settings-model";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewToggle from "../PreviewToggle.svelte";
  import CollectionEditor from "../CollectionEditor.svelte";
  import PasskeysGroup from "../PasskeysGroup.svelte";
</script>

<PasskeysGroup />
{#if showPreviews}
  <PreviewNotice />
  <SettingsGroup title={m.section_passwords()}
    ><PreviewToggle id="passwords.offer" /><PreviewToggle id="passwords.fill" /></SettingsGroup
  ><CollectionEditor id="passwords.entries" kind="credential" /><CollectionEditor
    id="passwords.addresses"
  />
{/if}

<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import * as preview from "../../lib/preview.svelte";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewAction from "../PreviewAction.svelte";
  import Field from "$shared/ui/Field";
  let folder = $state(preview.get("downloads.path", "Downloads"));
</script>

<PreviewNotice />
<SettingsGroup title={m.section_downloads()}
  ><PreviewAction
    id="downloads.path"
    actionLabel={m.settings_choose_folder()}
    value={preview.get("downloads.path", "Downloads")}
    onopen={() => {
      folder = preview.get("downloads.path", "Downloads");
    }}
    valid={folder.trim().length > 0}
    onapply={() => preview.set("downloads.path", folder.trim())}
    ><Field label={m.preview_folder_path()} bind:value={folder} required maxlength={512} />
    <p class="settings-help">{m.preview_folder_note()}</p></PreviewAction
  ><PreviewToggle id="downloads.ask" /></SettingsGroup
>
<SettingsGroup title={m.settings_downloads()}
  ><PreviewToggle id="downloads.notify" /><PreviewSelect id="downloads.clear" /></SettingsGroup
>

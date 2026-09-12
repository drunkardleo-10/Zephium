<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import { IS_MAC } from "$shared/platform";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Switch from "$shared/ui/Switch";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import CollectionEditor from "../CollectionEditor.svelte";
  const shortcuts = [
    { title: m.settings_shortcut_newtab, key: "T" },
    { title: m.settings_shortcut_search, key: "L" },
    { title: m.settings_shortcut_settings, key: "," },
    { title: m.settings_shortcut_close, key: "W" },
  ];
</script>

<SettingsGroup title={m.settings_keyboard()}
  >{#each shortcuts as shortcut (shortcut.key)}<SettingsRow title={shortcut.title()}
      ><kbd>{IS_MAC ? "⌘" : "Ctrl +"} {shortcut.key}</kbd></SettingsRow
    >{/each}</SettingsGroup
>
<SettingsGroup title={m.settings_motion()}
  ><SettingsRow title={m.settings_reduce_motion()} description={m.settings_reduce_motion_desc()}
    ><Switch
      label={m.settings_reduce_motion()}
      labelHidden
      checked={preferences.value("ui.reduce-motion") === "true"}
      disabled={preferences.saving()}
      onchange={(value) => void preferences.set("ui.reduce-motion", String(value))}
    /></SettingsRow
  ></SettingsGroup
>
<PreviewNotice /><CollectionEditor id="shortcuts.editor" kind="shortcut" /><SettingsGroup
  title={m.settings_accessibility()}
  ><PreviewSelect id="accessibility.text" /><PreviewToggle
    id="accessibility.contrast"
  /></SettingsGroup
>

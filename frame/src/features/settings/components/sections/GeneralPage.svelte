<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import * as settingsState from "../../lib/settings-state.svelte";
  import * as preview from "../../lib/preview.svelte";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewDialog from "../PreviewDialog.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewAction from "../PreviewAction.svelte";
  import CollectionEditor from "../CollectionEditor.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import Checkbox from "$shared/ui/Checkbox";
  import Select from "$shared/ui/Select";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import { Globe02Icon } from "@hugeicons/core-free-icons";
  let defaultOpen = $state(false);
</script>

<div class="settings-callout" data-setting="general.default">
  <span class="settings-callout-icon"><Icon icon={Globe02Icon} size={26} /></span>
  <div>
    <h2>{m.general_default_title()}</h2>
    <p>{m.general_default_body()}</p>
  </div>
  <Button variant="primary" onclick={() => (defaultOpen = true)}
    >{m.general_default_action()}</Button
  >
</div>
<PreviewNotice />
<SettingsGroup title={m.general_startup_title()}
  ><PreviewSelect id="general.startup" /><PreviewToggle id="general.restore" /></SettingsGroup
>
{#if preview.get("general.startup", "Continue where I left off") === "Open specific pages" || settingsState.highlighted() === "general.pages"}<CollectionEditor
    id="general.pages"
    kind="url"
  />{/if}
<SettingsGroup title={m.general_data_group()}>
  <PreviewAction id="general.import" actionLabel={m.general_import_button()}
    ><Select
      label={m.preview_import_source()}
      value={preview.get("import.source", "Safari")}
      options={["Safari", "Chrome", "Edge", "Firefox", "Arc", "Zen", "Bookmarks HTML"].map(
        (value) => ({ value, label: value }),
      )}
      onchange={(value) => preview.set("import.source", value)}
    />{#each [{ key: "bookmarks", label: m.preview_bookmarks }, { key: "history", label: m.preview_history }, { key: "passwords", label: m.preview_passwords }] as item (item.key)}<Checkbox
        label={item.label()}
        checked={preview.get(`import.${item.key}`, true)}
        onchange={(value) => preview.set(`import.${item.key}`, value)}
      />{/each}</PreviewAction
  >
  <PreviewAction id="general.export" actionLabel={m.general_export_button()}
    ><Select
      label={m.preview_export_format()}
      value={preview.get("export.format", "HTML")}
      options={["HTML", "JSON"].map((value) => ({ value, label: value }))}
      onchange={(value) => preview.set("export.format", value)}
    /><Checkbox
      label={m.preview_bookmarks()}
      checked={preview.get("export.bookmarks", true)}
      onchange={(value) => preview.set("export.bookmarks", value)}
    /><Checkbox
      label={m.preview_history()}
      checked={preview.get("export.history", false)}
      onchange={(value) => preview.set("export.history", value)}
    /></PreviewAction
  >
</SettingsGroup>
<PreviewDialog
  bind:open={defaultOpen}
  title={m.general_default_dialog()}
  description={m.general_default_note()}
  ><Checkbox
    label={m.general_default_choice()}
    checked={preview.get("general.default", false)}
    onchange={(value) => preview.set("general.default", value)}
  /></PreviewDialog
>

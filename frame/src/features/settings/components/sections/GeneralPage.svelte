<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import * as settingsState from "../../lib/settings-state.svelte";
  import * as preview from "../../lib/preview.svelte";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewAction from "../PreviewAction.svelte";
  import CollectionEditor from "../CollectionEditor.svelte";
  import ImportBrowserData from "../ImportBrowserData.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import Checkbox from "$shared/ui/Checkbox";
  import Select from "$shared/ui/Select";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import { Globe02Icon, Tick02Icon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import { defaultBrowser } from "$domain/default-browser";

  let status = $derived(defaultBrowser.current());

  // Windows answers in its own Settings window; coming back is the moment to
  // read the answer again.
  onMount(() => {
    void defaultBrowser.refresh();
    const reread = () => void defaultBrowser.refresh();
    window.addEventListener("focus", reread);
    return () => window.removeEventListener("focus", reread);
  });
</script>

<div class="settings-callout" data-setting="general.default">
  <span class="settings-callout-icon"
    ><Icon icon={status?.is_default ? Tick02Icon : Globe02Icon} size={26} /></span
  >
  <div>
    {#if status?.is_default}
      <h2>{m.general_default_done_title()}</h2>
      <p>{m.general_default_done_body()}</p>
    {:else}
      <h2>{m.general_default_title()}</h2>
      <p>
        {status && !status.can_request
          ? m.general_default_unavailable()
          : defaultBrowser.pending()
            ? m.general_default_asking()
            : m.general_default_body()}
      </p>
    {/if}
  </div>
  {#if status && !status.is_default && status.can_request}<Button
      variant="primary"
      disabled={defaultBrowser.pending()}
      onclick={() => void defaultBrowser.request()}>{m.general_default_action()}</Button
    >{/if}
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
  <ImportBrowserData />
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

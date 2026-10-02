<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { showPreviews } from "../../lib/settings-model";
  import type { HistoryRange } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import { tabs } from "$domain/tabs";
  import * as preview from "../../lib/preview.svelte";
  import PreviewNotice from "../PreviewNotice.svelte";
  import ProtectionControls from "../ProtectionControls.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewAction from "../PreviewAction.svelte";
  import SiteExceptions from "../SiteExceptions.svelte";
  import Checkbox from "$shared/ui/Checkbox";
  import Select from "$shared/ui/Select";

  // History deletion is real. Cookies and website cache still need the engine
  // data store, so those two stay labelled as preview.
  async function clear() {
    const profile = tabs.profile()?.id;
    if (!profile || !preview.get("clear.history", false)) return;
    const range = preview.get("clear.range", "hour") as HistoryRange;
    await commands.historyCall(profile, { kind: "clear", range });
  }
</script>

<ProtectionControls />
{#if showPreviews}
  <PreviewNotice />
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
{/if}
<SettingsGroup title={m.settings_browsing_data()}
  >{#if showPreviews}<PreviewToggle id="privacy.cleanup" />{/if}<PreviewAction
    id="privacy.clear"
    valid={["history", "cookies", "cache"].some((key) => preview.get(`clear.${key}`, false))}
    onapply={() => void clear()}
    ><Select
      label={m.preview_time_range()}
      value={preview.get("clear.range", "hour")}
      options={[
        { value: "hour", label: m.history_range_hour() },
        { value: "day", label: m.history_range_day() },
        { value: "week", label: m.history_range_week() },
        { value: "everything", label: m.history_range_all() },
      ]}
      onchange={(value) => preview.set("clear.range", value)}
    />{#each [{ key: "history", label: m.preview_history }, { key: "cookies", label: m.preview_cookies }, { key: "cache", label: m.preview_cache }] as item (item.key)}<Checkbox
        label={item.label()}
        checked={preview.get(`clear.${item.key}`, false)}
        onchange={(value) => preview.set(`clear.${item.key}`, value)}
      />{/each}</PreviewAction
  ></SettingsGroup
>

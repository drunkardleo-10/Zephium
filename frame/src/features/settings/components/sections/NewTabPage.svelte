<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import Switch from "$shared/ui/Switch";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
</script>

<div class="newtab-preview" aria-label={m.settings_preview()}>
  {#if preferences.value("ui.newtab-logo") === "true"}<span
      class="zephium-wordmark"
      role="img"
      aria-label="Zephium"
    ></span>{/if}
  <div class="preview-search" aria-hidden="true">{m.search_web()}</div>
  {#if preferences.value("ui.newtab-shortcuts") === "true"}<div
      class="preview-favorites"
      aria-hidden="true"
    >
      <i></i><i></i><i></i>
    </div>{/if}
</div>
<SettingsGroup title={m.settings_newtab_content()}>
  <SettingsRow
    settingId="ntp.logo"
    title={m.settings_wordmark()}
    description={m.settings_wordmark_desc()}
    ><Switch
      label={m.settings_wordmark()}
      labelHidden
      checked={preferences.value("ui.newtab-logo") === "true"}
      disabled={preferences.saving()}
      onchange={(v) => void preferences.set("ui.newtab-logo", String(v))}
    /></SettingsRow
  >
  <SettingsRow
    settingId="ntp.essentials"
    title={m.settings_favorites()}
    description={m.settings_favorites_desc()}
    ><Switch
      label={m.settings_favorites()}
      labelHidden
      checked={preferences.value("ui.newtab-shortcuts") === "true"}
      disabled={preferences.saving()}
      onchange={(v) => void preferences.set("ui.newtab-shortcuts", String(v))}
    /></SettingsRow
  >
</SettingsGroup>

<PreviewNotice /><SettingsGroup title={m.settings_newtab_content()}
  ><PreviewToggle id="ntp.greeting" /><PreviewToggle id="ntp.personalize" /><PreviewToggle
    id="ntp.clock"
  /><PreviewSelect id="ntp.clock-format" /><PreviewToggle id="ntp.continue" /></SettingsGroup
>

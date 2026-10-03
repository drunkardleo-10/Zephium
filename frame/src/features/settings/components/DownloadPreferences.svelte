<script lang="ts">
  import { untrack } from "svelte";
  import * as m from "$shared/i18n/messages";
  import { tabs } from "$domain/tabs";
  import { DownloadSession } from "$domain/downloads";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Button from "$shared/ui/Button";
  import Switch from "$shared/ui/Switch";
  let profile = $derived(tabs.profile()?.id ?? "unbound");
  let session = $state.raw(untrack(() => new DownloadSession(profile)));
  $effect(() => {
    const next = new DownloadSession(profile);
    session = next;
    void next.perform({ kind: "preferences" });
    return () => next.stop();
  });
</script>

{#if session.error}<p role="alert">
    {session.error === "unsupported"
      ? m.download_error_unsupported()
      : m.download_error_unavailable()}
  </p>{/if}
{#if !session.preferences && !session.error}<p role="status">
    {m.download_settings_loading()}
  </p>{/if}
<SettingsGroup title={m.section_downloads()}>
  <SettingsRow
    settingId="downloads.path"
    title={m.pref_downloads_path()}
    description={session.preferences?.directory ?? m.download_default_directory()}
  >
    <Button
      disabled={session.busy || !session.preferences || !session.supported}
      onclick={() => void session.perform({ kind: "choose_directory" })}
      >{m.settings_choose_folder()}</Button
    >
  </SettingsRow>
  <SettingsRow
    settingId="downloads.ask"
    title={m.pref_downloads_ask()}
    description={session.siteDownloadsRequireConfirmation
      ? m.download_windows_confirmation_help()
      : m.pref_downloads_ask_help()}
  >
    <Switch
      label={m.pref_downloads_ask()}
      labelHidden
      checked={session.siteDownloadsRequireConfirmation ||
        (session.preferences?.ask_destination ?? false)}
      disabled={session.busy ||
        !session.preferences ||
        !session.supported ||
        session.siteDownloadsRequireConfirmation}
      onchange={(enabled) => void session.perform({ kind: "set_ask_destination", enabled })}
    />
  </SettingsRow>
</SettingsGroup>

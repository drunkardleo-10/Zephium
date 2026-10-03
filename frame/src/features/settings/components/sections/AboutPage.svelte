<script lang="ts">
  import { onMount } from "svelte";
  import * as m from "$shared/i18n/messages";
  import { commands, type AboutInfo } from "$shared/ipc/bindings";
  import { preferences } from "$domain/preferences";
  import { keymap } from "$domain/keymap";
  import * as notices from "$session/notice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Button from "$shared/ui/Button";

  let about = $state<AboutInfo | null>(null);
  let confirming = $state(false);
  let resetting = $state(false);
  let outcome = $state<"done" | "failed" | null>(null);

  onMount(() => {
    let live = true;
    void commands
      .aboutInfo()
      .then((info) => {
        if (live) about = info;
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  });

  let platform = $derived(about ? m.about_platform({ os: about.os, arch: about.arch }) : "");

  async function copyDetails() {
    if (!about) return;
    const details = `Zephium ${about.version}\n${about.os} (${about.arch})`;
    try {
      await navigator.clipboard.writeText(details);
      notices.show(m.about_details_copied());
    } catch {
      // A denied clipboard leaves the details on screen to read.
    }
  }

  async function reset() {
    resetting = true;
    outcome = null;
    const settled = await preferences.resetAll();
    const shortcuts = await keymap.reset(null);
    resetting = false;
    confirming = false;
    outcome = settled && shortcuts ? "done" : "failed";
  }
</script>

<div class="settings-about">
  <span class="zephium-wordmark" role="img" aria-label="Zephium"></span>
  <p>{m.settings_about_body()}</p>
</div>
<SettingsGroup title={m.settings_about()}>
  <SettingsRow title={m.settings_version()} description={platform}>
    <span class="settings-value">{about?.version ?? ""}</span>
  </SettingsRow>
  <SettingsRow title={m.about_copy_details()} description={m.about_copy_details_help()}>
    <Button size="compact" disabled={!about} onclick={() => void copyDetails()}
      >{m.about_copy_details()}</Button
    >
  </SettingsRow>
</SettingsGroup>
<SettingsGroup title={m.settings_advanced()}>
  <SettingsRow
    settingId="about.reset"
    title={m.pref_about_reset()}
    description={outcome === "done"
      ? m.about_reset_done()
      : outcome === "failed"
        ? m.about_reset_failed()
        : m.pref_about_reset_help()}
  >
    <div class="reset">
      {#if confirming}
        <Button
          size="compact"
          variant="ghost"
          disabled={resetting}
          onclick={() => (confirming = false)}>{m.action_cancel()}</Button
        >
        <Button size="compact" variant="danger" pending={resetting} onclick={() => void reset()}
          >{m.about_reset_confirm()}</Button
        >
      {:else}
        <Button
          size="compact"
          onclick={() => {
            outcome = null;
            confirming = true;
          }}>{m.about_reset_action()}</Button
        >
      {/if}
    </div>
  </SettingsRow>
</SettingsGroup>

<style>
  .reset {
    display: flex;
    gap: 6px;
  }
</style>

<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { showPreviews } from "../../lib/settings-model";
  import * as preview from "../../lib/preview.svelte";
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewAction from "../PreviewAction.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import Checkbox from "$shared/ui/Checkbox";
  import Select from "$shared/ui/Select";
  import Icon from "$shared/ui/Icon";
  import { Tick02Icon, Download01Icon, Refresh01Icon } from "@hugeicons/core-free-icons";
  let version = $state("—");
  let status = $derived(preview.get("updates.status", "current"));
  const statuses = [
    { value: "current", label: m.updates_current },
    { value: "available", label: m.updates_available },
    { value: "failed", label: m.updates_failed },
  ];
  onMount(() => {
    let live = true;
    void getVersion()
      .then((value) => {
        if (live) version = value;
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  });
</script>

<div class="settings-about">
  <span class="zephium-wordmark" role="img" aria-label="Zephium"></span>
  <p>{m.settings_about_body()}</p>
  <span class="version-pill">{m.settings_version()} {version}</span>
</div>
{#if showPreviews}
  <PreviewNotice />
  <SettingsGroup title={m.updates_title()}
    ><PreviewAction id="updates.check" actionLabel={m.updates_check()}
      ><Select
        label={m.updates_preview_state()}
        value={status}
        options={statuses.map((item) => ({ value: item.value, label: item.label() }))}
        onchange={(value) => preview.set("updates.status", value)}
      />
      <div class="update-state">
        <span
          ><Icon
            icon={status === "current"
              ? Tick02Icon
              : status === "available"
                ? Download01Icon
                : Refresh01Icon}
            size={26}
          /></span
        >
        <h3>{statuses.find((item) => item.value === status)?.label()}</h3>
        <p>
          {status === "current"
            ? m.updates_current_body()
            : status === "available"
              ? m.updates_available_body()
              : m.updates_failed_body()}
        </p>
        <small>{m.updates_note()}</small>
      </div></PreviewAction
    ><PreviewToggle id="updates.automatic" /><PreviewSelect id="updates.channel" /></SettingsGroup
  >
{/if}
<SettingsGroup title={m.settings_application()}
  ><SettingsRow title={m.settings_version()}>{version}</SettingsRow><PreviewAction
    id="about.diagnostics"
    ><p>{m.preview_diagnostics_body()}</p>
    <pre class="diagnostic-preview">Zephium {version}</pre></PreviewAction
  ><PreviewAction
    id="about.reset"
    onapply={() =>
      preview.resetPrefixes([
        ...(preview.get("reset.appearance", true) ? ["appearance."] : []),
        ...(preview.get("reset.newtab", true) ? ["ntp."] : []),
      ])}
    ><p>{m.preview_reset_body()}</p>
    <Checkbox
      label={m.settings_appearance()}
      checked={preview.get("reset.appearance", true)}
      onchange={(value) => preview.set("reset.appearance", value)}
    /><Checkbox
      label={m.settings_newtab()}
      checked={preview.get("reset.newtab", true)}
      onchange={(value) => preview.set("reset.newtab", value)}
    /></PreviewAction
  ></SettingsGroup
>

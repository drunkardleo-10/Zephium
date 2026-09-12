<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import ChoiceGroup from "$shared/ui/ChoiceGroup";
  import Select from "$shared/ui/Select";
  import Switch from "$shared/ui/Switch";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Icon from "$shared/ui/Icon";
  import { Tick02Icon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  let material = $state("");
  onMount(() => {
    const update = () => {
      material = document.documentElement.dataset.material ?? "none";
    };
    update();
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-material"],
    });
    return () => observer.disconnect();
  });
  const tints = [
    { id: "graphite", label: m.settings_graphite },
    { id: "sky", label: m.settings_sky },
    { id: "sage", label: m.settings_sage },
    { id: "rose", label: m.settings_rose },
  ];
  const materials: Record<string, () => string> = {
    liquid_glass: m.settings_material_glass,
    vibrancy: m.settings_material_vibrancy,
    acrylic: m.settings_material_acrylic,
    mica: m.settings_material_mica,
    none: m.settings_material_solid,
  };
</script>

<SettingsGroup title={m.settings_theme()}>
  <div class="appearance-previews" aria-hidden="true">
    {#each ["light", "dark", "system"] as theme (theme)}
      <div
        class="appearance-preview"
        data-preview-theme={theme}
        data-selected={preferences.value("appearance") === theme}
      >
        <div class="mini-window">
          <div class="mini-sidebar"><span></span><i></i><i></i><i></i></div>
          <div class="mini-page"><span></span><i></i><i></i></div>
        </div>
      </div>
    {/each}
  </div>
  <SettingsRow
    settingId="appearance.theme"
    title={m.settings_color_scheme()}
    description={m.settings_color_scheme_desc()}
  >
    <ChoiceGroup
      label={m.settings_color_scheme()}
      segmented
      value={preferences.value("appearance")}
      disabled={preferences.saving()}
      options={[
        { value: "light", label: m.theme_light() },
        { value: "dark", label: m.theme_dark() },
        { value: "system", label: m.theme_system() },
      ]}
      onchange={(v) => void preferences.set("appearance", v)}
    />
  </SettingsRow>
  <SettingsRow
    settingId="appearance.accent"
    title={m.settings_accent()}
    description={m.settings_accent_desc()}
  >
    <div class="accent-choices" role="group" aria-label={m.settings_accent()}>
      {#each tints as tint (tint.id)}<button
          class="accent-swatch"
          style:--swatch={`var(--color-tint-${tint.id})`}
          aria-label={tint.label()}
          title={tint.label()}
          aria-pressed={preferences.value("ui.accent") === tint.id}
          disabled={preferences.saving()}
          onclick={() => void preferences.set("ui.accent", tint.id)}
          >{#if preferences.value("ui.accent") === tint.id}<Icon
              icon={Tick02Icon}
              size={14}
            />{/if}</button
        >{/each}
    </div>
  </SettingsRow>
</SettingsGroup>
<SettingsGroup title={m.settings_layout()}>
  <SettingsRow
    settingId="appearance.sidebar"
    title={m.settings_sidebar()}
    description={m.settings_sidebar_desc()}
  >
    <Select
      label={m.settings_sidebar()}
      labelHidden
      value={preferences.value("sidebar.mode")}
      disabled={preferences.saving()}
      options={[
        { value: "default", label: m.settings_expanded() },
        { value: "compact", label: m.settings_compact() },
      ]}
      onchange={(v) => void preferences.set("sidebar.mode", v)}
    />
  </SettingsRow>
  <SettingsRow
    settingId="appearance.material"
    title={m.settings_material()}
    description={m.settings_material_desc()}
    ><span class="settings-value">{materials[material]?.() ?? m.settings_material_solid()}</span
    ></SettingsRow
  >
</SettingsGroup>
<SettingsGroup title={m.settings_motion()}>
  <SettingsRow
    settingId="appearance.motion"
    title={m.settings_reduce_motion()}
    description={m.settings_reduce_motion_desc()}
    ><Switch
      label={m.settings_reduce_motion()}
      labelHidden
      checked={preferences.value("ui.reduce-motion") === "true"}
      disabled={preferences.saving()}
      onchange={(v) => void preferences.set("ui.reduce-motion", String(v))}
    /></SettingsRow
  >
</SettingsGroup>

<PreviewNotice /><SettingsGroup title={m.settings_layout()}
  ><PreviewSelect id="appearance.density" /><PreviewToggle id="appearance.icons" /></SettingsGroup
>

<SettingsGroup title={m.panel_tools()}
  ><SettingsRow title={m.panel_placement()} description={m.panel_placement_description()}
    ><Select
      label={m.panel_placement()}
      labelHidden
      value={preferences.value("tools.presentation")}
      disabled={preferences.saving()}
      options={[
        { value: "follow_layout", label: m.panel_follow_layout() },
        { value: "floating", label: m.panel_always_floating() },
      ]}
      onchange={(value) => void preferences.set("tools.presentation", value)}
    /></SettingsRow
  ></SettingsGroup
>

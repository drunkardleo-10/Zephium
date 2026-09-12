<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { tabs } from "$domain/tabs";
  import * as preview from "../../lib/preview.svelte";
  import PreviewNotice from "../PreviewNotice.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewDialog from "../PreviewDialog.svelte";
  import Button from "$shared/ui/Button";
  import Field from "$shared/ui/Field";
  import Slider from "$shared/ui/Slider";
  import Icon from "$shared/ui/Icon";
  import { UserCircleIcon, Globe02Icon, StarIcon, SparklesIcon } from "@hugeicons/core-free-icons";
  let failed = $state(false);
  let editing = $state(false);
  let newName = $state("");
  let input = $state<HTMLInputElement>();
  const icons = [UserCircleIcon, Globe02Icon, StarIcon, SparklesIcon];
  let name = $derived(preview.get("profiles.name", tabs.profile()?.name ?? ""));
  async function photo(event: Event) {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (!file) return;
    const scope = preview.scope();
    failed = false;
    if (
      !["image/png", "image/jpeg", "image/webp"].includes(file.type) ||
      file.size > 4 * 1024 * 1024
    ) {
      failed = true;
      return;
    }
    try {
      const data = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = reject;
        reader.readAsDataURL(file);
      });
      const image = new Image();
      image.src = data;
      await image.decode();
      if (image.naturalWidth > 4096 || image.naturalHeight > 4096) throw new Error("size");
      if (preview.scope() !== scope) return;
      preview.setPhoto(data);
      preview.set("profiles.icon", "Photo");
    } catch {
      failed = true;
    }
    if (input) input.value = "";
  }
</script>

<PreviewNotice />
<div class="profile-editor-hero">
  <div class="profile-photo-preview" aria-label={m.profile_image_preview()}>
    {#if preview.get("profiles.icon", "Initials") === "Photo" && preview.photo()}<img
        src={preview.photo()!}
        alt={m.profile_image_preview()}
        style:transform={`scale(${preview.get("profile.zoom", "1")})`}
        style:object-position={`${preview.get("profile.x", "50")}% ${preview.get("profile.y", "50")}%`}
      />{:else if preview.get("profiles.icon", "Initials") === "Icon"}<Icon
        icon={icons[Number(preview.get("profile.symbol", "0"))] ?? UserCircleIcon}
        size={40}
      />{:else}<span>{Array.from(name.trim())[0]?.toLocaleUpperCase() ?? "Z"}</span>{/if}
  </div>
  <div>
    <h2>{name || m.settings_current_profile()}</h2>
    <p>{m.settings_local_profile()}</p>
  </div>
</div>
<SettingsGroup title={m.section_profiles()}
  ><div data-setting="profiles.name">
    <SettingsRow title={m.settings_profile_name()}
      ><Field
        label={m.settings_profile_name()}
        labelHidden
        value={name}
        maxlength={80}
        oninput={(event) => preview.set("profiles.name", event.currentTarget.value)}
      /></SettingsRow
    >
  </div>
  <PreviewSelect id="profiles.icon" /></SettingsGroup
>
{#if preview.get("profiles.icon", "Initials") === "Icon"}<div class="profile-icon-choices">
    {#each icons as icon, index (index)}<button
        type="button"
        aria-label={`${m.settings_profile_name()} ${index + 1}`}
        aria-pressed={preview.get("profile.symbol", "0") === String(index)}
        onclick={() => preview.set("profile.symbol", String(index))}
        ><Icon {icon} size={25} /></button
      >{/each}
  </div>{/if}
{#if preview.get("profiles.icon", "Initials") === "Photo"}<div class="settings-inline-form">
    <input
      class="sr-only"
      tabindex="-1"
      bind:this={input}
      type="file"
      accept="image/png,image/jpeg,image/webp"
      onchange={photo}
    /><Button onclick={() => input?.click()}>{m.profile_choose_photo()}</Button>
    <p class="settings-help">{m.profile_photo_help()}</p>
    {#if failed}<p role="alert" class="settings-help" data-error="true">
        {m.profile_photo_invalid()}
      </p>{/if}{#if preview.photo()}{#each [{ key: "zoom", label: m.profile_zoom, min: 1, max: 3, step: 0.05, initial: "1" }, { key: "x", label: m.profile_horizontal, min: 0, max: 100, step: 1, initial: "50" }, { key: "y", label: m.profile_vertical, min: 0, max: 100, step: 1, initial: "50" }] as control (control.key)}<div
          class="profile-range"
        >
          <Slider
            label={control.label()}
            min={control.min}
            max={control.max}
            step={control.step}
            value={Number(preview.get(`profile.${control.key}`, control.initial))}
            onchange={(value) => preview.set(`profile.${control.key}`, String(value))}
          />
        </div>{/each}{/if}
  </div>{/if}
<SettingsGroup title={m.profile_preview_list()}
  >{#each preview.entries("profiles.list") as profile (profile.id)}<SettingsRow title={profile.name}
      ><Button variant="ghost" onclick={() => preview.removeEntry("profiles.list", profile.id)}
        >{m.preview_remove()}</Button
      ></SettingsRow
    >{/each}
  <div class="settings-inline-form">
    <Button onclick={() => (editing = true)}>{m.profile_create()}</Button>
  </div></SettingsGroup
>
<PreviewDialog
  bind:open={editing}
  title={m.profile_create()}
  valid={newName.trim().length > 0}
  onapply={() => {
    preview.saveEntry("profiles.list", {
      id: crypto.randomUUID(),
      name: newName.trim(),
      value: "",
    });
    newName = "";
  }}
  ><Field
    label={m.profile_new_name()}
    bind:value={newName}
    required
    maxlength={80}
  /></PreviewDialog
>

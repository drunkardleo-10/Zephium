<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import { keymap, type KeymapEntry, type KeymapOutcome } from "$domain/keymap";
  import { IS_MAC } from "$shared/platform";
  import { acceleratorFrom, acceleratorKeys, heldModifiers } from "$shared/lib/accelerator";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Switch from "$shared/ui/Switch";
  import Button from "$shared/ui/Button";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import LauncherShortcut from "../LauncherShortcut.svelte";
  import ShortcutField from "../ShortcutField.svelte";
  import { commandTitle, keymapSections } from "../../lib/keymap-model";

  type Notice = { id: string; text: string; replace?: string };

  let sections = $derived(keymapSections(keymap.all()));
  let anyCustomized = $derived(keymap.all().some((entry) => entry.customized));
  let recordingId = $state<string | null>(null);
  let held = $state<string[]>([]);
  let notice = $state<Notice | null>(null);

  function titleOf(id: string) {
    const entry = keymap.all().find((candidate) => candidate.id === id);
    return entry ? commandTitle(entry) : id;
  }

  function explain(outcome: KeymapOutcome, accelerator: string | null): Omit<Notice, "id"> | null {
    switch (outcome.kind) {
      case "applied":
        return null;
      case "conflict":
        return {
          text: m.keymap_conflict({ command: titleOf(outcome.command) }),
          replace: accelerator ?? undefined,
        };
      case "unbindable":
        return { text: IS_MAC ? m.keymap_unbindable_mac() : m.keymap_unbindable() };
      case "reserved":
        return { text: m.keymap_reserved() };
      case "invalid":
      case "fixed":
        return { text: m.keymap_invalid() };
      case "unavailable":
        return { text: m.keymap_failed() };
    }
  }

  async function bind(id: string, accelerator: string | null, replace = false) {
    const outcome = await keymap.bind(id, accelerator, replace);
    const problem = explain(outcome, accelerator);
    notice = problem ? { id, ...problem } : null;
  }

  async function reset(id: string | null) {
    notice = null;
    if (!(await keymap.reset(id))) notice = id ? { id, text: m.keymap_failed() } : null;
  }

  function begin(id: string) {
    notice = null;
    held = [];
    recordingId = id;
    void keymap.record(true);
  }

  function end() {
    if (recordingId === null) return;
    recordingId = null;
    held = [];
    void keymap.record(false);
  }

  // Capture on the window runs before the browser's own shortcut handler, so
  // a chord being recorded never also runs the command it currently names.
  function keydown(event: KeyboardEvent) {
    const id = recordingId;
    if (id === null) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    const bare = !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
    if (bare && event.key === "Escape") {
      end();
      return;
    }
    if (bare && (event.key === "Backspace" || event.key === "Delete")) {
      end();
      void bind(id, null);
      return;
    }
    const accelerator = acceleratorFrom(event, IS_MAC);
    if (accelerator === null) {
      held = heldModifiers(event, IS_MAC);
      return;
    }
    end();
    void bind(id, accelerator);
  }

  function keyup(event: KeyboardEvent) {
    if (recordingId !== null) held = heldModifiers(event, IS_MAC);
  }

  $effect(() => {
    if (recordingId === null) return;
    const options = { capture: true };
    window.addEventListener("keydown", keydown, options);
    window.addEventListener("keyup", keyup, options);
    window.addEventListener("blur", end);
    return () => {
      window.removeEventListener("keydown", keydown, options);
      window.removeEventListener("keyup", keyup, options);
      window.removeEventListener("blur", end);
    };
  });

  $effect(() => () => end());

  function rowDescription(entry: KeymapEntry) {
    if (recordingId === entry.id) return m.keymap_recording_help();
    if (notice?.id === entry.id) return notice.text;
    return undefined;
  }
</script>

<LauncherShortcut />
{#if sections.length > 0}
  <SettingsGroup title={m.settings_keyboard()}>
    <SettingsRow title={m.keymap_reset_all()} description={m.keymap_reset_all_help()}>
      <Button size="compact" disabled={!anyCustomized} onclick={() => void reset(null)}
        >{m.keymap_reset_all_action()}</Button
      >
    </SettingsRow>
  </SettingsGroup>
{/if}
{#each sections as section (section.id)}
  <SettingsGroup title={section.title}>
    {#each section.entries as entry (entry.id)}
      {@const title = commandTitle(entry)}
      {@const recording = recordingId === entry.id}
      {@const replace = notice?.id === entry.id ? notice.replace : undefined}
      <SettingsRow {title} description={rowDescription(entry)} settingId={`keymap.${entry.id}`}>
        <div class="shortcut">
          {#if replace !== undefined && !recording}<Button
              variant="secondary"
              size="compact"
              onclick={() => void bind(entry.id, replace, true)}>{m.keymap_replace()}</Button
            >{:else if entry.customized && !recording}<Button
              variant="ghost"
              size="compact"
              onclick={() => void reset(entry.id)}>{m.keymap_reset()}</Button
            >{/if}
          <ShortcutField
            keys={entry.accelerator ? acceleratorKeys(entry.accelerator, IS_MAC) : []}
            {held}
            {recording}
            prompt={m.keymap_press()}
            empty={m.keymap_unbound()}
            warn={notice?.id === entry.id}
            label={m.keymap_record_label({ command: title })}
            onclick={() => (recording ? end() : begin(entry.id))}
          />
        </div>
      </SettingsRow>
    {/each}
  </SettingsGroup>
{/each}
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
<PreviewNotice /><SettingsGroup title={m.settings_accessibility()}
  ><PreviewSelect id="accessibility.text" /><PreviewToggle
    id="accessibility.contrast"
  /></SettingsGroup
>

<style>
  .shortcut {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
</style>

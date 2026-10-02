<script lang="ts">
  import { preferences, type PreferenceKey } from "$domain/preferences";
  import { fields, type FieldId } from "../lib/catalog";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Select from "$shared/ui/Select";

  /** `id` names the row (labels, options, search); `preference` is what it
   *  changes. The catalog's option values are the preference's own values. */
  let { id, preference }: { id: FieldId; preference: PreferenceKey } = $props();
  let field = $derived(fields[id]);
  let options = $derived(
    "options" in field
      ? field.options.map((option) => ({ value: option.value, label: option.label() }))
      : [],
  );
</script>

<SettingsRow settingId={id} title={field.label()} description={field.description()}
  ><Select
    label={field.label()}
    labelHidden
    value={preferences.value(preference)}
    disabled={preferences.saving()}
    {options}
    onchange={(value) => void preferences.set(preference, value)}
  /></SettingsRow
>

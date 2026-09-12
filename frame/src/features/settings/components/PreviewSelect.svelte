<script lang="ts">
  import { fields, type FieldId } from "../lib/catalog";
  import * as preview from "../lib/preview.svelte";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Select from "$shared/ui/Select";
  let { id }: { id: FieldId } = $props();
  let field = $derived(fields[id]);
  let options = $derived(
    "options" in field
      ? field.options.map((option) => ({ value: option.value, label: option.label() }))
      : [],
  );
</script>

<div data-setting={id}>
  <SettingsRow title={field.label()} description={field.description()}
    ><Select
      label={field.label()}
      labelHidden
      value={String(preview.get(id, field.initial))}
      {options}
      onchange={(value) => preview.set(id, value)}
    /></SettingsRow
  >
</div>

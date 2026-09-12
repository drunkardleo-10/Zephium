<script lang="ts">
  import { fields, type FieldId } from "../lib/catalog";
  import * as preview from "../lib/preview.svelte";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Switch from "$shared/ui/Switch";
  let { id }: { id: FieldId } = $props();
  let field = $derived(fields[id]);
</script>

<div data-setting={id}>
  <SettingsRow title={field.label()} description={field.description()}
    ><Switch
      label={field.label()}
      labelHidden
      checked={Boolean(preview.get(id, field.initial))}
      onchange={(value) => preview.set(id, value)}
    /></SettingsRow
  >
</div>

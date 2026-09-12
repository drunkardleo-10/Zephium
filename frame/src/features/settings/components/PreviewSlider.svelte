<script lang="ts">
  import { fields, type FieldId } from "../lib/catalog";
  import * as preview from "../lib/preview.svelte";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Slider from "$shared/ui/Slider";
  let { id, label, format }: { id: FieldId; label?: string; format?: (value: number) => string } =
    $props();
  let field = $derived(fields[id]);
  let range = $derived("range" in field ? field.range : { min: 0, max: 100, step: 1 });
</script>

<div data-setting={id}>
  <SettingsRow title={field.label()} description={field.description()}
    ><div class="control">
      <Slider
        label={label ?? field.label()}
        min={range.min}
        max={range.max}
        step={range.step}
        value={Number(preview.get(id, String(field.initial)))}
        {format}
        onchange={(value) => preview.set(id, String(value))}
      />
    </div></SettingsRow
  >
</div>

<style>
  .control {
    width: 220px;
  }
</style>

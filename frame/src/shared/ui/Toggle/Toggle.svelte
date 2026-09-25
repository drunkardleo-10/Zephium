<script lang="ts">
  import Switch from "../Switch/Switch.svelte";
  import Checkbox from "../Checkbox/Checkbox.svelte";
  let {
    label,
    labelHidden = false,
    description,
    checked = $bindable(false),
    disabled = false,
    kind = "switch",
    onchange,
  }: {
    label: string;
    labelHidden?: boolean;
    description?: string;
    checked?: boolean;
    disabled?: boolean;
    kind?: "switch" | "checkbox";
    onchange?: (checked: boolean) => void;
  } = $props();
  function change(next: boolean) {
    checked = next;
    onchange?.(next);
  }
</script>

{#if kind === "switch"}<Switch
    {label}
    {labelHidden}
    {description}
    {checked}
    {disabled}
    onchange={change}
  />{:else}<Checkbox
    {label}
    {labelHidden}
    {description}
    {checked}
    {disabled}
    onchange={change}
  />{/if}

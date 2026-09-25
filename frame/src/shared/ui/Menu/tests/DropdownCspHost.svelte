<script lang="ts">
  import Menu from "../Menu.svelte";
  import Select from "$shared/ui/Select";
  let choice = $state("first");
  let clicks = $state(0);
</script>

<Menu
  label="Actions"
  entries={[{ kind: "item", id: "choose", label: "Choose action" }]}
  onselect={() => clicks++}>{#snippet trigger()}Actions{/snippet}</Menu
>
<Select
  label="Choice"
  bind:value={choice}
  options={[
    { value: "first", label: "First" },
    { value: "second", label: "Second" },
  ]}
/>
<button onclick={() => clicks++}>After dropdown</button>
<output>Clicks: {clicks}</output>

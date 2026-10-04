<script lang="ts">
  import Menu from "../Menu.svelte";
  import Select from "$shared/ui/Select";
  import Disclosure from "$shared/ui/Disclosure";
  let { opaque = false }: { opaque?: boolean } = $props();
</script>

<div data-menu-material={opaque ? "opaque" : undefined}>
  <Menu label="Actions" entries={[{ kind: "item", id: "one", label: "One" }]} onselect={() => {}}>
    {#snippet trigger()}Actions{/snippet}
  </Menu>
  <Select label="Choice" value="one" options={[{ value: "one", label: "One" }]} />
  <Disclosure label="Utilities">
    {#snippet trigger()}Utilities{/snippet}
    <button>Panel action</button>
  </Disclosure>
</div>

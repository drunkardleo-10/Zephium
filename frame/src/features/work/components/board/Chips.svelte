<script lang="ts">
  import EvidenceChips from "$shared/ui/data/Artifact/EvidenceChips.svelte";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  let {
    keys,
    sources,
    onopen,
  }: {
    keys: readonly string[];
    sources: Readonly<Record<string, EvidenceReference>>;
    onopen?: (reference: EvidenceReference) => void;
  } = $props();
  const references = $derived(keys.flatMap((key) => (sources[key] ? [sources[key]] : [])));
</script>

{#if references.length}<span class="chips nodrag nopan"
    ><EvidenceChips {references} compact onevidence={onopen} /></span
  >{/if}

<style>
  .chips {
    display: inline-flex;
    vertical-align: 1px;
    margin-inline-start: 6px;
  }
</style>

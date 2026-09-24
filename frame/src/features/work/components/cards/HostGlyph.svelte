<script lang="ts">
  // The implementation file, not the Artifact module: a small lazy chunk that
  // draws one glyph must not pull the whole result renderer with it.
  import HostGlyph from "$shared/ui/data/Artifact/HostGlyph.svelte";
  let { host, file, size }: { host?: string; file?: boolean; size?: number } = $props();
</script>

<HostGlyph {host} {file} {size} />

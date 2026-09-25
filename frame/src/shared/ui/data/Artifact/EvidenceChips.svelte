<script lang="ts">
  import type { EvidenceReference } from "./artifact";
  import HostGlyph from "./HostGlyph.svelte";
  let {
    references,
    compact = false,
    onevidence,
  }: {
    references: readonly EvidenceReference[];
    compact?: boolean;
    onevidence?: (reference: EvidenceReference) => void;
  } = $props();
  /** A chip is the site's mark and its host, or the file by its name; never a number. */
  const name = (reference: EvidenceReference) =>
    reference.file ? reference.label : reference.origin || reference.label;
</script>

{#if references.length}
  <span class="chips" class:compact>
    {#each references as reference (reference.key)}
      <button
        type="button"
        class="chip"
        disabled={!onevidence}
        title={reference.label}
        onclick={() => onevidence?.(reference)}
      >
        <HostGlyph
          host={reference.origin || reference.label}
          url={reference.url}
          file={!!reference.file}
          size={compact ? 12 : 14}
        />
        <span class="name">{name(reference)}</span>
      </button>
    {/each}
  </span>
{/if}

<style>
  .chips {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-inline-size: 180px;
    padding: 1px 8px 1px 2px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: var(--text-caption);
    line-height: 14px;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  .name {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .compact .chip {
    max-inline-size: 132px;
    padding: 1px 6px 1px 2px;
  }

  .chip:disabled {
    cursor: default;
  }

  .chip:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>

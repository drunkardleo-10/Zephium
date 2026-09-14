<script lang="ts">
  import type { EvidenceReference } from "./artifact";
  let {
    references,
    compact = false,
    onevidence,
  }: {
    references: readonly EvidenceReference[];
    compact?: boolean;
    onevidence?: (reference: EvidenceReference) => void;
  } = $props();
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
        {#if compact}{reference.origin || reference.label}{:else}{reference.label}{/if}
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
    max-inline-size: 180px;
    padding: 2px 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: var(--text-caption);
    line-height: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  .compact .chip {
    max-inline-size: 120px;
    padding: 1px 6px;
    font-size: 10px;
  }

  .chip:disabled {
    cursor: default;
  }

  .chip:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>

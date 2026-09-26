<script lang="ts">
  import type { StatBlock } from "../../lib/board/types";
  let { block }: { block: StatBlock } = $props();
</script>

<p class="label">{block.label}</p>
<p class="value">
  {block.value}{#if block.unit}<span class="unit">{block.unit}</span>{/if}
</p>
{#if block.basis}<p class="basis">{block.basis}</p>{/if}

<style>
  p {
    margin: 0;
  }

  .label {
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .value {
    margin-block-start: 4px;
    font-size: var(--text-title);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
  }

  .unit {
    margin-inline-start: 4px;
    color: var(--color-muted);
    font-size: var(--text-body);
    font-weight: 500;
  }

  .basis {
    margin-block-start: 6px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }
</style>

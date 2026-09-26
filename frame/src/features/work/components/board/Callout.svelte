<script lang="ts">
  import type { CalloutBlock } from "../../lib/board/types";
  let { block }: { block: CalloutBlock } = $props();
</script>

<p class="callout {block.tone}">{block.text}</p>

<style>
  .callout {
    margin: 0;
    padding-inline-start: 12px;
    border-inline-start: 2px solid var(--color-border-strong);
    color: var(--color-label-secondary);
    font-size: var(--text-body);
    line-height: 20px;
  }

  .caution {
    border-inline-start-color: var(--color-warning);
  }

  .decision {
    border-inline-start-color: var(--color-lit);
    color: var(--color-text);
  }
</style>

<script lang="ts">
  import { inline } from "./inline";
  /** `plain` sets bold as the destination would (a draft), without the reply's accent. */
  let { text, plain = false }: { text: string; plain?: boolean } = $props();
</script>

{#each inline(text) as run, index (index)}{#if run.strong}<strong class:plain>{run.text}</strong
    >{:else if run.code}<code>{run.text}</code>{:else}{run.text}{/if}{/each}

<style>
  /* A term the text leans on reads in a soft accent, never a highlight. */
  strong {
    color: var(--color-term);
    font-weight: 600;
  }

  strong.plain {
    color: inherit;
  }

  code {
    padding: 1px 5px;
    border-radius: var(--radius-inset);
    background: var(--color-code-chip);
    color: var(--color-code-chip-ink);
    font-family: var(--font-mono);
    font-size: 0.86em;
    box-decoration-break: clone;
  }
</style>

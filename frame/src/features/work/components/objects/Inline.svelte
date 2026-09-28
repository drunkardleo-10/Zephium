<script lang="ts">
  import { inline } from "./inline";
  let { text }: { text: string } = $props();
</script>

{#each inline(text) as run, index (index)}{#if run.strong}<strong>{run.text}</strong
    >{:else if run.code}<code>{run.text}</code>{:else}{run.text}{/if}{/each}

<style>
  strong {
    color: var(--color-text);
    font-weight: 600;
  }

  code {
    padding: 1px 5px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: 0.88em;
  }
</style>

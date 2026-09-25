<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    children,
    title,
    retryLabel,
    onFailure,
  }: {
    children: Snippet;
    title: string;
    retryLabel: string;
    onFailure?: () => void;
  } = $props();
</script>

<svelte:boundary onerror={() => onFailure?.()}>
  {@render children()}
  {#snippet failed(_error, reset)}
    <div class="failure" role="alert">
      <p>{title}</p>
      <button type="button" onclick={reset}>{retryLabel}</button>
    </div>
  {/snippet}
</svelte:boundary>

<style>
  .failure {
    display: grid;
    justify-items: start;
    gap: 12px;
    padding: 16px;
    color: var(--color-text);
  }

  p {
    margin: 0;
  }

  button {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-row);
    padding: 6px 12px;
    background: var(--color-fill);
    color: var(--color-text);
    font: inherit;
  }

  button:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
</style>

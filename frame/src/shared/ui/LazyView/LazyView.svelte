<script lang="ts" generics="P extends Record<string, unknown>">
  import type { Component, Snippet } from "svelte";
  import RenderBoundary from "../RenderBoundary";
  let {
    loader,
    children,
    loadingLabel,
    failureLabel,
    retryLabel,
  }: {
    loader: () => Promise<{ default: Component<P> }>;
    children: Snippet<[Component<P>]>;
    loadingLabel: string;
    failureLabel: string;
    retryLabel: string;
  } = $props();
  let component = $state.raw<Component<P> | null>(null);
  let failed = $state(false);
  let attempt = $state(0);
  $effect(() => {
    const load = loader;
    void attempt;
    let current = true;
    component = null;
    failed = false;
    void Promise.resolve()
      .then(() => (current ? load() : null))
      .then(
        (module) => {
          if (current && module) component = module.default;
        },
        () => {
          if (current) failed = true;
        },
      );
    return () => {
      current = false;
    };
  });
</script>

{#if failed}
  <div class="load-state" role="alert">
    <p>{failureLabel}</p>
    <button type="button" onclick={() => attempt++}>{retryLabel}</button>
  </div>
{:else if component}
  <RenderBoundary title={failureLabel} {retryLabel}>
    {@render children(component)}
  </RenderBoundary>
{:else}
  <div class="load-state" role="status">{loadingLabel}</div>
{/if}

<style>
  .load-state {
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
    border-radius: var(--radius-sm);
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

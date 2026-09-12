<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    label,
    children,
    floating = false,
    class: className = "",
  }: { label: string; children: Snippet; floating?: boolean; class?: string } = $props();
</script>

<div role="group" aria-label={label} class={["ui-cluster", className]} data-floating={floating}>
  {@render children()}
</div>

<style>
  /* One capsule holding several controls, so a toolbar reads as one shape.
     Floating clusters carry the only soft shadow ordinary chrome gets. */
  .ui-cluster {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    box-sizing: border-box;
    padding: 3px;
    border-radius: var(--radius-capsule);
    background: var(--color-control);
    box-shadow: var(--shadow-control);
  }

  .ui-cluster[data-floating="true"] {
    background: var(--color-menu);
    box-shadow: var(--shadow-float);
  }

  .ui-cluster > :global(.ui-chip:not([aria-pressed="true"], :hover, :active)) {
    background: transparent;
    box-shadow: none;
  }

  @media (forced-colors: active) {
    .ui-cluster {
      border: 1px solid ButtonText;
      box-shadow: none;
    }
  }
</style>

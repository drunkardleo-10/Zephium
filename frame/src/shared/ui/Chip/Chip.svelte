<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    label,
    selected,
    disabled = false,
    leading,
    onclick,
  }: {
    label: string;
    selected?: boolean;
    disabled?: boolean;
    leading?: Snippet;
    onclick?: () => void;
  } = $props();
</script>

<button
  type="button"
  class="ui-chip"
  aria-pressed={selected === undefined ? undefined : selected}
  {disabled}
  {onclick}
>
  {#if leading}{@render leading()}{/if}{label}
</button>

<style>
  /* A chip is one of the few things that genuinely is a pill — it is a tag,
     not a control in a column. Selected takes the same soft plate and rim as
     every other chosen thing in the product. */
  .ui-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    min-height: 28px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-control);
    color: var(--color-on-control);
    font: inherit;
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    box-shadow: var(--shadow-control);
    cursor: default;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    transition:
      color var(--motion-instant) var(--ease-smooth),
      background-color var(--motion-instant) var(--ease-smooth),
      box-shadow var(--motion-instant) var(--ease-smooth),
      scale var(--motion-slow) var(--ease-smooth);
  }

  .ui-chip:disabled {
    opacity: 0.5;
  }

  .ui-chip:hover:not(:disabled) {
    background: var(--color-control-hover);
    color: var(--color-on-control-strong);
  }

  .ui-chip:active:not(:disabled) {
    background: var(--color-control-pressed);
    scale: 0.96;
    transition-duration: var(--motion-fast);
  }

  .ui-chip[aria-pressed="true"] {
    background: var(--row-active);
    color: var(--color-text);
    box-shadow: var(--row-rim);
  }

  @media (forced-colors: active) {
    .ui-chip {
      border: 1px solid ButtonText;
    }

    .ui-chip[aria-pressed="true"] {
      background: Highlight;
      color: HighlightText;
    }
  }
</style>

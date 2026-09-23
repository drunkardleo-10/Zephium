<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    label,
    description,
    leading,
    trailing,
    selected = false,
    disabled = false,
    onclick,
  }: {
    label: string;
    description?: string;
    leading?: Snippet;
    trailing?: Snippet;
    selected?: boolean;
    disabled?: boolean;
    onclick?: () => void;
  } = $props();
</script>

<button type="button" class="ui-list-row" aria-pressed={selected} {disabled} {onclick}>
  {#if leading}<span class="leading">{@render leading()}</span>{/if}<span class="copy"
    ><span>{label}</span>{#if description}<small>{description}</small>{/if}</span
  >{#if trailing}<span class="trailing">{@render trailing()}</span>{/if}
</button>

<style>
  .ui-list-row {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    width: 100%;
    min-height: 32px;
    padding: 5px 8px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-muted);
    text-align: start;
    font: inherit;
    cursor: default;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    transition:
      color var(--motion-fast) var(--ease-out),
      background-color var(--motion-fast) var(--ease-out);
  }

  .ui-list-row:disabled {
    opacity: 0.45;
  }

  .ui-list-row:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .ui-list-row[aria-pressed="true"] {
    background: var(--color-fill-active);
    color: var(--color-text);
    box-shadow: var(--shadow-control);
  }

  .copy {
    display: grid;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .copy > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .copy small {
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .leading {
    display: grid;
    place-items: center;
    flex: none;
    width: 22px;
    height: 22px;
    color: var(--color-faint);
  }

  .ui-list-row[aria-pressed="true"] .leading,
  .ui-list-row:hover .leading {
    color: var(--color-text);
  }

  .trailing {
    color: var(--color-faint);
    font-size: var(--text-label);
  }

  @media (forced-colors: active) {
    .ui-list-row[aria-pressed="true"] {
      outline: 1px solid Highlight;
    }
  }
</style>

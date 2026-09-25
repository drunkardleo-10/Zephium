<script lang="ts">
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "$shared/ui/Icon";
  let {
    icon,
    label,
    active = false,
    prominent = false,
    disabled = false,
    id,
    onclick,
    ...rest
  }: {
    icon: IconSvgElement;
    label: string;
    active?: boolean;
    prominent?: boolean;
    disabled?: boolean;
    id?: string;
    onclick?: (event: MouseEvent) => void;
    [key: string]: unknown;
  } = $props();
</script>

<button
  {id}
  type="button"
  class="tool"
  class:active
  class:prominent
  {disabled}
  aria-pressed={active || undefined}
  {onclick}
  {...rest}
>
  <span class="glyph"><Icon {icon} size={18} strokeWidth={1.6} /></span>
  <span class="label">{label}</span>
</button>

<style>
  .tool {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    box-sizing: border-box;
    min-inline-size: 58px;
    block-size: 46px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-label-secondary);
    font: inherit;
    cursor: default;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      color var(--motion-fast) var(--ease-smooth),
      scale var(--motion-base) var(--ease-spring);
  }

  .tool:disabled {
    color: var(--color-faint);
  }

  .tool:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .tool:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .tool:active:not(:disabled) {
    scale: 0.96;
  }

  .tool.active {
    background: var(--color-fill-active);
    color: var(--color-text);
  }

  .tool.prominent {
    background: var(--color-control);
    color: var(--color-text);
    box-shadow: var(--shadow-control);
    margin-inline: 4px;
  }

  .tool.prominent:hover:not(:disabled) {
    background: var(--color-control-hover);
  }

  .tool.prominent.active {
    background: var(--color-control-pressed);
  }

  .glyph {
    display: grid;
    place-items: center;
    block-size: 20px;
  }

  .label {
    font-size: 10.5px;
    font-weight: 500;
    line-height: 12px;
    letter-spacing: 0.01em;
    white-space: nowrap;
  }
</style>

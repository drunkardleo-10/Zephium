<script lang="ts">
  import { Checkbox } from "bits-ui";
  import { Tick02Icon, MinusSignIcon } from "@hugeicons/core-free-icons";
  import Icon from "../Icon/Icon.svelte";
  let {
    label,
    description,
    labelHidden = false,
    checked = false,
    indeterminate = false,
    disabled = false,
    onchange,
  }: {
    label: string;
    description?: string;
    labelHidden?: boolean;
    checked?: boolean;
    indeterminate?: boolean;
    disabled?: boolean;
    onchange?: (checked: boolean) => void;
  } = $props();
  const uid = $props.id();
</script>

<div class="control">
  <Checkbox.Root
    id={uid}
    class="ui-checkbox"
    {checked}
    {indeterminate}
    {disabled}
    onCheckedChange={onchange}
    aria-label={label}
    aria-describedby={description ? `${uid}-help` : undefined}
  >
    {#snippet children({ checked, indeterminate })}{#if checked || indeterminate}<span
          class="mark"
          aria-hidden="true"
          ><Icon
            icon={indeterminate ? MinusSignIcon : Tick02Icon}
            size={13}
            strokeWidth={2.4}
          /></span
        >{/if}{/snippet}
  </Checkbox.Root>
  <div class="copy" class:sr-only={labelHidden}>
    <label for={uid}>{label}</label>{#if description}<p id={`${uid}-help`} class="help">
        {description}
      </p>{/if}
  </div>
</div>

<style>
  /* Unchecked is the control fill. Checked is the lit rung, flat: in a product
     with no accent hue the box that is on is simply the brightest thing on the
     row, and a gradient inside a 16px square is detail nobody can see that
     still costs the shape its crispness. */
  .control {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .copy label {
    font-size: var(--text-body);
    line-height: 20px;
  }

  .help {
    display: block;
    margin: 0;
    font-size: var(--text-label);
    line-height: 1.6;
    color: var(--color-muted);
  }

  .control :global(.ui-checkbox) {
    display: grid;
    place-items: center;
    flex: none;
    box-sizing: border-box;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 0;

    /* A quarter of its own side. The radius roles start at 24px controls;
       nothing else in the product is this small. */
    border-radius: 5px;
    overflow: hidden;
    background: var(--color-control);
    color: var(--color-on-lit);
    box-shadow: var(--shadow-control);
    cursor: default;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      box-shadow var(--motion-instant) var(--ease-smooth),
      scale var(--motion-slow) var(--ease-smooth);
  }

  .control :global(.ui-checkbox[data-state="unchecked"]:hover:not(:disabled)) {
    background: var(--color-control-hover);
  }

  .control :global(.ui-checkbox:active:not(:disabled)) {
    scale: 0.94;
    transition-duration: var(--motion-fast);
  }

  .control :global(.ui-checkbox[data-state="checked"]),
  .control :global(.ui-checkbox[data-state="indeterminate"]) {
    background: var(--color-lit);
    box-shadow: none;
  }

  .mark {
    display: grid;
    place-items: center;
    animation: mark-in var(--motion-slow) var(--ease-spring);
  }

  @keyframes mark-in {
    from {
      opacity: 0;
      transform: scale(0.4);
    }
  }

  .control :global(.ui-checkbox:disabled) {
    opacity: 0.5;
  }

  .control :global(.ui-checkbox:focus-visible) {
    outline: 2px solid var(--color-ring);
    outline-offset: 3px;
  }

  @media (forced-colors: active) {
    .control :global(.ui-checkbox) {
      border: 1px solid ButtonText;
    }

    .control :global(.ui-checkbox[data-state="checked"]),
    .control :global(.ui-checkbox[data-state="indeterminate"]) {
      background: Highlight;
      color: HighlightText;
    }
  }
</style>

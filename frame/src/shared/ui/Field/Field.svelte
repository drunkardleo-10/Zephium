<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";
  let {
    label,
    labelHidden = false,
    hint,
    error,
    value = $bindable(""),
    class: className = "",
    ...rest
  }: Omit<HTMLInputAttributes, "value"> & {
    label: string;
    labelHidden?: boolean;
    hint?: string;
    error?: string;
    value?: string;
  } = $props();
  const uid = $props.id();
</script>

<div class={["ui-field", className]}>
  <label for={uid} class:sr-only={labelHidden}>{label}</label>
  <input
    {...rest}
    id={uid}
    bind:value
    aria-invalid={error ? true : undefined}
    aria-describedby={error || hint ? `${uid}-help` : undefined}
    class="ui-input"
  />
  {#if error || hint}<p id={`${uid}-help`} class="ui-help" data-error={!!error}>
      {error || hint}
    </p>{/if}
</div>

<style>
  /* A field is fill at rest and fill plus a ring while editing: the ring is
     what says the caret is in here, so it cannot also be the thing that says
     the field exists. */
  .ui-field {
    display: grid;
    gap: 6px;
    min-width: 0;
  }

  label {
    font-size: var(--text-label);
    font-weight: 500;
    color: var(--color-muted);
  }

  .ui-input {
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    min-height: var(--field-height);
    padding: 5px 14px;
    border: 0;
    border-radius: var(--radius-control);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    line-height: 20px;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      box-shadow var(--motion-instant) var(--ease-smooth);
  }

  .ui-input::placeholder {
    color: var(--color-faint);
  }

  .ui-input:focus {
    outline: none;
    background: var(--color-field-hover);
    box-shadow: var(--shadow-field-focus);
  }

  .ui-input:disabled {
    opacity: 0.5;
  }

  .ui-input:hover:not(:disabled, :focus) {
    background: var(--color-field-hover);
  }

  .ui-input[aria-invalid="true"] {
    box-shadow: inset 0 0 0 1px var(--color-danger);
  }

  .ui-help {
    display: block;
    margin: 0;
    font-size: var(--text-label);
    line-height: 1.6;
    color: var(--color-muted);
  }

  .ui-help[data-error="true"] {
    color: var(--color-danger);
  }

  @media (forced-colors: active) {
    .ui-input {
      border: 1px solid ButtonText;
      box-shadow: none;
    }

    .ui-input[aria-invalid="true"] {
      outline: 2px dashed ButtonText;
    }
  }
</style>

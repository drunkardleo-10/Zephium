<script lang="ts">
  import type { HTMLTextareaAttributes } from "svelte/elements";
  let {
    label,
    hint,
    error,
    value = $bindable(""),
    ...rest
  }: Omit<HTMLTextareaAttributes, "value"> & {
    label: string;
    hint?: string;
    error?: string;
    value?: string;
  } = $props();
  const uid = $props.id();
</script>

<div class="ui-field">
  <label for={uid}>{label}</label>
  <textarea
    rows="3"
    {...rest}
    id={uid}
    bind:value
    class="ui-input"
    aria-invalid={error ? true : undefined}
    aria-describedby={error || hint ? `${uid}-help` : undefined}></textarea>
  {#if error || hint}<p id={`${uid}-help`} class="ui-help" data-error={!!error}>
      {error || hint}
    </p>{/if}
</div>

<style>
  /* Fields sit level with the surface; a ring appears only while editing. */
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
    min-height: 88px;
    padding: 8px 12px;
    resize: vertical;
    border: 0;
    border-radius: var(--radius-control);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    line-height: 1.5;
    box-shadow: var(--shadow-field);
    transition:
      background-color var(--motion-base) var(--ease-smooth),
      box-shadow var(--motion-base) var(--ease-smooth);
  }

  .ui-input::placeholder {
    color: var(--color-faint);
  }

  .ui-input:focus {
    outline: none;
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

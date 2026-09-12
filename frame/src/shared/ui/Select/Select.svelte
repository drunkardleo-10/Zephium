<script lang="ts">
  import { Select } from "bits-ui";
  import "../Menu/popover.css";
  let {
    label,
    labelHidden = false,
    options,
    value = $bindable(""),
    disabled = false,
    align = "end",
    onchange,
  }: {
    label: string;
    labelHidden?: boolean;
    options: ReadonlyArray<{ value: string; label: string; disabled?: boolean }>;
    value?: string;
    disabled?: boolean;
    align?: "start" | "center" | "end";
    onchange?: (value: string) => void;
  } = $props();
  const uid = $props.id();
  let current = $derived(options.find((option) => option.value === value)?.label ?? "");
</script>

<div class="field">
  <label for={uid} class:sr-only={labelHidden}>{label}</label>
  <Select.Root
    type="single"
    bind:value
    {disabled}
    items={[...options]}
    onValueChange={(next) => onchange?.(next)}
  >
    <Select.Trigger id={uid} class="ui-select" aria-label={label}>
      <span class="value">{current}</span>
      <svg
        class="chevron"
        aria-hidden="true"
        viewBox="0 0 12 12"
        width="12"
        height="12"
        fill="none"
        stroke="currentColor"
      >
        <path
          d="M3 4.5l3 3 3-3"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </Select.Trigger>
    <Select.Portal>
      <Select.Content class="ui-menu" {align} sideOffset={6}>
        <Select.Viewport class="ui-menu-viewport">
          {#each options as option (option.value)}
            <Select.Item
              class="ui-menu-item"
              value={option.value}
              label={option.label}
              disabled={option.disabled}
            >
              {#snippet children({ selected })}
                {option.label}
                <span class="ui-menu-check" aria-hidden="true"
                  >{#if selected}<svg
                      viewBox="0 0 12 12"
                      width="12"
                      height="12"
                      fill="none"
                      stroke="currentColor"
                      ><path
                        d="M2.5 6.5l2.5 2.5 4.5-5"
                        stroke-width="1.8"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                      /></svg
                    >{/if}</span
                >
              {/snippet}
            </Select.Item>
          {/each}
        </Select.Viewport>
      </Select.Content>
    </Select.Portal>
  </Select.Root>
</div>

<style>
  /* A pop-up button: the current value on a quiet field, one chevron at the
     end. The menu carries the weight. */
  .field {
    display: grid;
    gap: 6px;
    min-width: 0;
  }

  .field > label {
    font-size: var(--text-label);
    font-weight: 500;
    color: var(--color-muted);
  }

  .field :global(.ui-select) {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    box-sizing: border-box;
    min-width: 120px;
    min-height: var(--field-height);
    padding: 5px 10px 5px 13px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    font-weight: 450;
    line-height: 16px;
    box-shadow: var(--shadow-field);
    cursor: default;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    transition:
      background-color var(--motion-base) var(--ease-smooth),
      box-shadow var(--motion-base) var(--ease-smooth);
  }

  .field :global(.ui-select:disabled) {
    opacity: 0.5;
  }

  .field :global(.ui-select:hover:not(:disabled)),
  .field :global(.ui-select[data-state="open"]) {
    background: var(--color-field-hover);
  }

  .value {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    text-align: start;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chevron {
    flex: none;
    color: var(--color-muted);
  }

  @media (forced-colors: active) {
    .field :global(.ui-select) {
      border: 1px solid ButtonText;
    }
  }
</style>

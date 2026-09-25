<script lang="ts">
  import { Select } from "bits-ui";
  import Mark from "../Menu/Mark.svelte";
  import "../Menu/popover.css";
  let {
    label,
    labelHidden = false,
    options,
    value = $bindable(""),
    disabled = false,
    onchange,
  }: {
    label: string;
    labelHidden?: boolean;
    options: ReadonlyArray<{ value: string; label: string; disabled?: boolean }>;
    value?: string;
    disabled?: boolean;
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
        viewBox="0 0 12 16"
        width="12"
        height="16"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path d="M3 6.5L6 3.5l3 3" />
        <path d="M3 9.5l3 3 3-3" />
      </svg>
    </Select.Trigger>
    <Select.Portal>
      <Select.Content
        class="ui-menu ui-select-menu"
        align="start"
        sideOffset={6}
        collisionPadding={12}
      >
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
                <Mark on={selected} />
              {/snippet}
            </Select.Item>
          {/each}
        </Select.Viewport>
      </Select.Content>
    </Select.Portal>
  </Select.Root>
</div>

<style>
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
    padding: 5px 8px 5px 14px;
    border: 0;

    /* Almost round, and no ring. A hairline around a control this small draws
       a shape the fill has already drawn, and two edges a pixel apart is what
       makes a field look like a widget rather than a surface. The text inset
       matches the menu's own, so the label does not move when it opens. */
    border-radius: var(--radius-field);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    font-weight: 450;
    line-height: 16px;
    cursor: default;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      box-shadow var(--motion-instant) var(--ease-smooth);
  }

  .field :global(.ui-select:disabled) {
    opacity: 0.5;
  }

  .field :global(.ui-select:hover:not(:disabled)) {
    background: var(--color-field-hover);
  }

  /* While its menu is up the trigger is held rather than hovered — and it is
     usually underneath the panel anyway, since the chosen row opens over it. */
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

  /* Two chevrons, because this one picks from a set rather than opening a
     list of commands — the platform's own distinction, and the fastest way to
     tell the two apart without reading either. */
  .chevron {
    flex: none;
    color: var(--color-muted);
  }

  /* At least the trigger's width, so the label it is covering never has to
     move sideways to fit. */
  :global(.ui-select-menu) {
    min-width: var(--bits-floating-anchor-width, 0);
  }

  @media (forced-colors: active) {
    .field :global(.ui-select) {
      border: 1px solid ButtonText;
    }
  }
</style>

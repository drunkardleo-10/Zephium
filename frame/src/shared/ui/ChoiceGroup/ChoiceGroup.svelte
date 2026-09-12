<script lang="ts">
  let {
    label,
    options,
    value = $bindable(""),
    segmented = false,
    full = false,
    disabled = false,
    onchange,
  }: {
    label: string;
    options: ReadonlyArray<{ value: string; label: string; disabled?: boolean }>;
    value?: string;
    segmented?: boolean;
    full?: boolean;
    disabled?: boolean;
    onchange?: (value: string) => void;
  } = $props();
  const uid = $props.id();
  let selectedIndex = $derived(options.findIndex((option) => option.value === value));
</script>

<fieldset class="ui-choices" data-segmented={segmented} data-full={full} {disabled}>
  <legend class:sr-only={segmented}>{label}</legend>
  <div
    class="options"
    style:--segment-count={options.length}
    style:--segment-index={Math.max(0, selectedIndex)}
  >
    {#if segmented}<span class="indicator" aria-hidden="true" data-visible={selectedIndex >= 0}
      ></span>{/if}
    {#each options as option (option.value)}<label
        ><input
          type="radio"
          name={uid}
          value={option.value}
          bind:group={value}
          disabled={option.disabled}
          onchange={() => onchange?.(option.value)}
        /><span>{option.label}</span></label
      >{/each}
  </div>
</fieldset>

<style>
  /* Segmented: quiet buttons in a row; the chosen one carries a raised fill
     that slides between them. Plain: native radios in a row. */
  .ui-choices {
    border: 0;
    padding: 0;
    margin: 0;
    min-width: 0;
  }

  legend {
    margin-bottom: 8px;
    font-size: var(--text-label);
    font-weight: 500;
    color: var(--color-muted);
  }

  .options {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
  }

  label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-body);
  }

  input {
    accent-color: var(--color-accent);
  }

  input:focus-visible + span {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  input:disabled + span {
    opacity: 0.5;
  }

  .ui-choices[data-segmented="true"] .options {
    position: relative;
    display: inline-grid;
    grid-template-columns: repeat(var(--segment-count), minmax(0, 1fr));
    gap: 4px;
    box-sizing: border-box;
  }

  .ui-choices[data-full="true"],
  .ui-choices[data-full="true"] .options {
    display: grid;
    width: 100%;
  }

  .ui-choices[data-segmented="true"] label {
    position: relative;
    z-index: 1;
    justify-content: center;
  }

  .ui-choices[data-segmented="true"] input {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: 0;
    opacity: 0;
    pointer-events: none;
  }

  .ui-choices[data-segmented="true"] label > span {
    box-sizing: border-box;
    width: 100%;
    min-height: var(--control-regular);
    padding: 8px 12px;
    border-radius: var(--radius-control);
    color: var(--color-muted);
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 16px;
    text-align: center;
    white-space: nowrap;
    transition:
      color var(--motion-base) var(--ease-smooth),
      background-color var(--motion-base) var(--ease-smooth);
  }

  .ui-choices[data-segmented="true"] label:hover > span {
    background: var(--color-control);
    color: var(--color-text);
  }

  .ui-choices[data-segmented="true"] input:checked + span {
    background: transparent;
    color: var(--color-text);
  }

  .indicator {
    position: absolute;
    display: block;
    inset-block: 0;
    inset-inline-start: 0;
    width: calc((100% - 4px * (var(--segment-count) - 1)) / var(--segment-count));
    border-radius: var(--radius-control);
    background: var(--color-raise);
    box-shadow: var(--shadow-raise);
    transform: translateX(calc(var(--segment-index) * (100% + 4px)));
    transition: transform var(--motion-slow) var(--ease-out);
    pointer-events: none;
  }

  .indicator[data-visible="false"] {
    visibility: hidden;
  }

  :global([dir="rtl"]) .indicator {
    transform: translateX(calc(var(--segment-index) * (-100% - 4px)));
  }

  @media (forced-colors: active) {
    .ui-choices[data-segmented="true"] input:checked + span,
    .indicator {
      outline: 1px solid Highlight;
    }
  }
</style>

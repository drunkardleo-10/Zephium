<script lang="ts">
  let {
    label,
    value = $bindable(0),
    min = 0,
    max = 100,
    step = 1,
    disabled = false,
    format = (current: number) => String(current),
    onchange,
    oncommit,
  }: {
    label: string;
    value?: number;
    min?: number;
    max?: number;
    step?: number;
    disabled?: boolean;
    format?: (value: number) => string;
    onchange?: (value: number) => void;
    oncommit?: (value: number) => void;
  } = $props();
  const uid = $props.id();
  let dragging = $state(false);
  let editing = $state(false);
  let draft = $state("");
  let field: HTMLDivElement;
  let input = $state<HTMLInputElement>();
  let decimals = $derived(Math.max(0, (String(step).split(".")[1] ?? "").length));
  let ratio = $derived(max > min ? (Math.min(max, Math.max(min, value)) - min) / (max - min) : 0);
  let text = $derived(format(value));

  function quantize(next: number): number {
    const clamped = Math.min(max, Math.max(min, next));
    return Number((Math.round((clamped - min) / step) * step + min).toFixed(decimals));
  }
  function set(next: number) {
    const quantized = quantize(next);
    if (quantized === value) return;
    value = quantized;
    onchange?.(quantized);
  }
  function fromPointer(event: PointerEvent) {
    const rect = field.getBoundingClientRect();
    const position = (event.clientX - rect.left) / rect.width;
    const along = getComputedStyle(field).direction === "rtl" ? 1 - position : position;
    set(min + Math.min(1, Math.max(0, along)) * (max - min));
  }
  function down(event: PointerEvent) {
    if (disabled || editing || event.button !== 0) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragging = true;
    fromPointer(event);
  }
  function move(event: PointerEvent) {
    if (dragging) fromPointer(event);
  }
  function up() {
    if (!dragging) return;
    dragging = false;
    oncommit?.(value);
  }
  function key(event: KeyboardEvent) {
    if (disabled) return;
    const jump = event.shiftKey ? step * 10 : step;
    const moves: Record<string, number> = {
      ArrowRight: value + jump,
      ArrowUp: value + jump,
      ArrowLeft: value - jump,
      ArrowDown: value - jump,
      PageUp: value + step * 10,
      PageDown: value - step * 10,
      Home: min,
      End: max,
    };
    if (event.key === "Enter") {
      edit();
      return;
    }
    const next = moves[event.key];
    if (next === undefined) return;
    event.preventDefault();
    set(next);
    oncommit?.(value);
  }
  function edit() {
    if (disabled) return;
    draft = String(value);
    editing = true;
  }
  function commit() {
    if (!editing) return;
    editing = false;
    const parsed = Number(draft.trim());
    if (Number.isFinite(parsed)) {
      set(parsed);
      oncommit?.(value);
    }
  }
  function editKey(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      commit();
    } else if (event.key === "Escape") {
      event.preventDefault();
      editing = false;
    }
  }
  $effect(() => {
    if (editing) input?.select();
  });
</script>

<div
  class="field"
  bind:this={field}
  data-dragging={dragging}
  data-editing={editing}
  data-disabled={disabled}
  style:--ratio={ratio}
>
  <span class="fill" aria-hidden="true"><span class="thumb"></span></span>
  <div
    class="track"
    role="slider"
    tabindex={disabled ? -1 : 0}
    aria-label={label}
    aria-valuemin={min}
    aria-valuemax={max}
    aria-valuenow={value}
    aria-valuetext={text}
    aria-disabled={disabled || undefined}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onkeydown={key}
  ></div>
  <span class="label" id={`${uid}-label`}>{label}</span>
  {#if editing}
    <input
      class="input"
      bind:this={input}
      bind:value={draft}
      type="text"
      inputmode="decimal"
      aria-labelledby={`${uid}-label`}
      onkeydown={editKey}
      onblur={commit}
    />
  {:else}
    <button type="button" class="value" tabindex="-1" {disabled} onclick={edit}>{text}</button>
  {/if}
</div>

<style>
  /* A value field: the label at the start, the value at the end, and the
     amount drawn as a raised fill that grows from the left. Drag anywhere to
     scrub; click the value to type. */
  .field {
    position: relative;
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    height: var(--field-height, 32px);
    border-radius: var(--radius-control);
    background: var(--color-field);
    box-shadow: var(--shadow-field);
    overflow: hidden;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    transition:
      background-color var(--motion-base) var(--ease-smooth),
      box-shadow var(--motion-base) var(--ease-smooth);
  }

  .field:hover:not([data-editing="true"], [data-disabled="true"]) {
    background: var(--color-field-hover);
  }

  .field[data-editing="true"] {
    box-shadow: var(--shadow-field-focus);
  }

  .field[data-disabled="true"] {
    opacity: 0.5;
  }

  .fill {
    position: absolute;
    inset-block: 0;
    inset-inline-start: 0;
    width: max(32px, calc(var(--ratio) * 100%));
    border-radius: var(--radius-control);
    background: var(--color-raise);
    box-shadow: var(--shadow-raise);
    pointer-events: none;
    transition:
      background-color var(--motion-base) var(--ease-smooth),
      opacity var(--motion-base) var(--ease-smooth),
      width 220ms var(--ease-out);
  }

  .field[data-dragging="true"] .fill {
    background: var(--color-raise-active);
    transition-duration: var(--motion-fast), var(--motion-fast), 0ms;
  }

  .field[data-editing="true"] .fill {
    opacity: 0;
  }

  .thumb {
    position: absolute;
    inset-inline-end: 6px;
    top: 7px;
    width: 2px;
    height: 18px;
    border-radius: 2px;
    background: var(--color-muted);
    opacity: 0;
    transition: opacity var(--motion-base) var(--ease-smooth);
  }

  .field:hover .thumb {
    opacity: 0.6;
  }

  .field[data-dragging="true"] .thumb {
    opacity: 1;
  }

  .track {
    position: absolute;
    inset: 0;
    z-index: 2;
    border-radius: inherit;
    cursor: ew-resize;
    touch-action: none;
    outline: none;
  }

  .track:focus-visible {
    box-shadow: inset 0 0 0 2px var(--color-ring);
  }

  .field[data-disabled="true"] .track {
    cursor: default;
  }

  .label {
    position: absolute;
    inset-inline-start: 12px;
    top: 50%;
    transform: translateY(-50%);
    z-index: 3;
    font-size: var(--text-body);
    font-weight: 500;
    color: var(--color-muted);
    pointer-events: none;
  }

  .value,
  .input {
    position: absolute;
    inset-inline-end: 0;
    inset-block: 0;
    z-index: 3;
    margin: 0;
    padding: 0 12px;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    font-variant-numeric: tabular-nums;
    text-align: end;
  }

  .value {
    cursor: text;
  }

  .input {
    z-index: 4;
    width: 40%;
    outline: none;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: text;
    user-select: text;
  }

  @media (forced-colors: active) {
    .field {
      border: 1px solid ButtonText;
    }

    .fill {
      background: Highlight;
    }
  }
</style>

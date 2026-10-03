<script lang="ts">
  let {
    keys,
    held = [],
    recording,
    prompt,
    empty,
    label,
    warn = false,
    disabled = false,
    onclick,
  }: {
    /** The keycaps currently bound, already in the platform's order. */
    keys: string[];
    /** Modifiers held while recording, before a key completes the chord. */
    held?: string[];
    recording: boolean;
    prompt: string;
    /** Shown when nothing is bound. */
    empty?: string;
    label: string;
    warn?: boolean;
    disabled?: boolean;
    onclick: () => void;
  } = $props();

  let shown = $derived(recording ? held : keys);
</script>

<!-- The keycaps are the control: no box around them until it is listening. -->
<button
  type="button"
  class="recorder"
  class:recording
  class:warn={warn && !recording}
  {disabled}
  aria-label={label}
  aria-pressed={recording}
  {onclick}
>
  {#if recording && !shown.length}<span class="prompt">{prompt}</span>
  {:else if !shown.length}<span class="prompt">{empty ?? prompt}</span>
  {:else}{#each shown as key, i (i)}<kbd>{key}</kbd>{/each}{/if}
</button>

<style>
  .recorder {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: 3px;
    height: 30px;
    padding: 0 4px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      box-shadow var(--motion-fast) var(--ease-smooth),
      scale var(--motion-slow) var(--ease-smooth);
  }

  .recorder:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .recorder:hover:not(:disabled) {
    background: var(--row-hover);
  }

  .recorder:active:not(:disabled) {
    scale: 0.97;
    transition-duration: var(--motion-fast);
  }

  .recorder.recording {
    min-width: 108px;
    justify-content: center;
    padding: 0 8px;
    background: var(--color-field);
    box-shadow:
      inset 0 0 0 1px var(--color-ring),
      0 0 0 3px var(--color-field-ring);
  }

  .recorder.warn {
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-warning) 70%, transparent);
  }

  .prompt {
    padding: 0 4px;
    color: var(--color-muted);
    font-size: 12.5px;
  }

  kbd {
    display: inline-grid;
    place-items: center;
    box-sizing: border-box;
    min-width: 24px;
    height: 24px;
    padding: 0 7px;
    border-radius: var(--radius-inset);
    background: var(--color-control);
    box-shadow: var(--shadow-control);
    color: var(--color-text);
    font-family: var(--font-sans);
    font-size: 12px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .recording kbd {
    color: var(--color-muted);
  }

  @media (prefers-reduced-motion: reduce) {
    .recorder {
      transition: none;
    }

    .recorder:active:not(:disabled) {
      scale: none;
    }
  }

  @media (forced-colors: active) {
    .recorder.recording {
      outline: 2px solid Highlight;
    }

    kbd {
      border: 1px solid CanvasText;
    }
  }
</style>

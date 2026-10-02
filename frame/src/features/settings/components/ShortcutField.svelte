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
    justify-content: center;
    gap: 4px;
    min-width: 112px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-control);
    background: var(--color-field);
    color: var(--color-text);
    transition:
      border-color var(--motion-fast) var(--ease-smooth),
      box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .recorder:hover:not(:disabled) {
    background: var(--color-field-hover);
  }

  .recorder.recording {
    border-color: var(--color-ring);
    box-shadow: 0 0 0 3px var(--color-field-ring);
  }

  .recorder.warn {
    border-color: var(--color-warning);
  }

  .prompt {
    font-size: 12.5px;
    color: var(--color-muted);
  }

  kbd {
    display: inline-grid;
    place-items: center;
    box-sizing: border-box;
    min-width: 22px;
    height: 22px;
    padding: 0 6px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    font-family: var(--font-sans);
    font-size: 12px;
    font-weight: 500;
  }

  @media (prefers-reduced-motion: reduce) {
    .recorder {
      transition: none;
    }
  }

  @media (forced-colors: active) {
    .recorder.recording {
      outline: 2px solid Highlight;
    }
  }
</style>

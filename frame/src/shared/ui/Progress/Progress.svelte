<script lang="ts">
  let { label, value, max = 100 }: { label: string; value?: number; max?: number } = $props();
  const uid = $props.id();
</script>

<div class="ui-progress">
  <label for={uid}>{label}</label><progress id={uid} {value} {max}></progress>
</div>

<style>
  /* A quiet track with a raised fill; indeterminate keeps the native motion. */
  .ui-progress {
    display: grid;
    gap: 8px;
  }

  label {
    font-size: var(--text-label);
    font-weight: 500;
    color: var(--color-muted);
  }

  progress {
    width: 100%;
    height: 6px;
    border: 0;
    border-radius: var(--radius-capsule);
    overflow: hidden;
    background: var(--color-track);
    accent-color: var(--color-accent);
  }

  progress::-webkit-progress-bar {
    background: transparent;
  }

  progress::-webkit-progress-value {
    border-radius: var(--radius-capsule);
    background: var(--color-accent);
    transition: inline-size var(--motion-slow) var(--ease-out);
  }

  progress::-moz-progress-bar {
    border-radius: var(--radius-capsule);
    background: var(--color-accent);
  }

  @media (forced-colors: active) {
    progress {
      forced-color-adjust: auto;
    }
  }
</style>

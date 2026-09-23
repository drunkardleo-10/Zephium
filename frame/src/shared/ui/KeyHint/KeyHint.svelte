<script lang="ts">
  let { keys, label }: { keys: string[]; label?: string } = $props();
</script>

<span class="ui-keys" aria-label={label}
  >{#each keys as key, i (i)}<kbd>{key}</kbd>{/each}</span
>

<style>
  .ui-keys {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: var(--radius-inset);
    background: var(--color-control);
    color: var(--color-muted);
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
</style>

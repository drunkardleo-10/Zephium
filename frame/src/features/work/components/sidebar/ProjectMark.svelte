<script lang="ts">
  let { title, current = false }: { title: string; current?: boolean } = $props();
  const letter = $derived([...title.trim()][0]?.toLocaleUpperCase() ?? "");
</script>

<!-- A project has no favicon; its first letter stands where a tab's icon would. -->
<span class="mark" data-current={current} aria-hidden="true">{letter}</span>

<style>
  .mark {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 18px;
    block-size: 18px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font-size: var(--text-caption);
    font-weight: 600;
    line-height: 1;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .mark[data-current="true"] {
    background: var(--color-fill-active);
    color: var(--color-text);
  }
</style>

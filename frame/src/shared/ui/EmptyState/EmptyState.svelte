<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    title,
    description,
    icon,
    action,
  }: { title: string; description: string; icon?: Snippet; action?: Snippet } = $props();
</script>

<div class="ui-empty">
  {#if icon}<div class="icon">{@render icon()}</div>{/if}
  <h3>{title}</h3>
  <p>{description}</p>
  {#if action}{@render action()}{/if}
</div>

<style>
  .ui-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    text-align: center;
    gap: 10px;
    padding: 40px 24px;
  }

  h3 {
    margin: 0;
    font-size: 15px;
    font-weight: 550;
    letter-spacing: -0.01em;
  }

  p {
    max-width: 36ch;
    margin: 0 0 8px;
    line-height: 1.6;
    color: var(--color-muted);
  }

  .icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    margin-bottom: 4px;
    border-radius: var(--radius-card);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-faint);
  }
</style>

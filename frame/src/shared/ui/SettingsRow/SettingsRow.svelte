<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    title,
    description,
    children,
    settingId,
  }: { title: string; description?: string; children: Snippet; settingId?: string } = $props();
</script>

<div class="ui-settings-row" data-setting={settingId}>
  <div class="ui-settings-copy">
    <h3>{title}</h3>
    {#if description}<p>{description}</p>{/if}
  </div>
  <div class="ui-settings-control">{@render children()}</div>
</div>

<style>
  .ui-settings-row {
    position: relative;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 24px;
    box-sizing: border-box;
    min-height: var(--row-page);
    padding: 10px 16px;
  }

  :global(.ui-settings-row) + .ui-settings-row::before {
    content: "";
    position: absolute;
    inset-inline: 16px;
    top: 0;
    height: 1px;
    background: var(--color-border);
  }

  .ui-settings-copy {
    min-width: 0;
  }

  h3 {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-page-title);
    font-weight: 500;
    line-height: 19px;
    letter-spacing: -0.008em;
  }

  p {
    margin: 2px 0 0;
    font-size: var(--text-label);
    line-height: 1.5;
    color: var(--color-muted);
  }

  .ui-settings-control {
    flex: none;
    max-width: 58%;
  }
</style>

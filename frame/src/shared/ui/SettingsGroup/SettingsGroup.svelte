<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, description, children }: { title: string; description?: string; children: Snippet } =
    $props();
  const uid = $props.id();
</script>

<section class="ui-settings-section" aria-labelledby={uid}>
  <h2 id={uid}>{title}</h2>
  <div class="ui-settings-group">{@render children()}</div>
  {#if description}<p class="ui-settings-note">{description}</p>{/if}
</section>

<style>
  /* A grouped card; rows share one surface and are divided by inset
     hairlines drawn by the rows themselves. */
  .ui-settings-section {
    margin-block: 0 28px;
  }

  h2 {
    margin: 0 0 8px;
    padding-inline: 16px;
    color: var(--color-muted);
    font-size: var(--text-label);
    font-weight: 500;
  }

  .ui-settings-group {
    overflow: hidden;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: 0 0 0 1px var(--color-border);
  }

  .ui-settings-note {
    margin: 8px 16px 0;
    font-size: var(--text-label);
    line-height: 1.5;
    color: var(--color-muted);
  }

  @media (forced-colors: active) {
    .ui-settings-group {
      border: 1px solid ButtonText;
      box-shadow: none;
    }
  }
</style>

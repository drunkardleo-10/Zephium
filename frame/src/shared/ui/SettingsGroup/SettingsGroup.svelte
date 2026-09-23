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
    margin-block: 0 34px;
  }

  /* The heading is the page's skeleton, so it carries the page's voice: a
     small grey caption over a card leaves the page with no structure to
     read at a glance. It aligns to the row label rather than the card edge,
     which is what makes a group look set into the page instead of on it. */
  h2 {
    margin: 0 0 10px;
    padding-inline: 16px;
    color: var(--color-text);
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    letter-spacing: -0.012em;
  }

  /* No outline: the card now steps a real distance off the page, and a
     hairline on top of a luminance step reads as weight rather than edge. */
  .ui-settings-group {
    overflow: hidden;
    border-radius: var(--radius-card);
    background: var(--color-surface);
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

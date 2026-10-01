<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * One question put to the person: what, where, the one decision. On the
   * canvas it is a raised sheet beside the thing it concerns; in the island it
   * takes the island's own material.
   */
  let {
    placement = "canvas",
    label,
    mark,
    where,
    title,
    children,
    note = null,
    actions,
    busy = false,
    fill = false,
  }: {
    placement?: "canvas" | "island";
    /** The accessible name of the question as a whole. */
    label: string;
    mark: Snippet;
    /** Where it happens: "Slack · as you". */
    where: string;
    title: string;
    children?: Snippet;
    /** Fine print above the actions: what exactly will happen. */
    note?: string | null;
    actions: Snippet;
    busy?: boolean;
    /** The actions row is one control across the card, such as an answer field. */
    fill?: boolean;
  } = $props();
</script>

<section class="ask {placement}" aria-label={label} aria-busy={busy || undefined}>
  <header>
    <span class="mark" aria-hidden="true">{@render mark()}</span>
    <span class="where">{where}</span>
  </header>
  <h3>{title}</h3>
  {#if children}<div class="body">{@render children()}</div>{/if}
  {#if note}<p class="note">{note}</p>{/if}
  <footer class:fill>{@render actions()}</footer>
</section>

<style>
  .ask {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    min-inline-size: 0;
    color: var(--color-text);
    font-size: var(--text-body);
  }

  /* A sheet that has just been set down: it rises once and then holds still. */
  .canvas {
    inline-size: 360px;
    padding: 14px 16px;
    border-radius: var(--radius-card);
    background: var(--color-raised);
    box-shadow: var(--shadow-float);
    animation: ask-in var(--motion-slow) var(--ease-emphasized);
  }

  .island {
    inline-size: 100%;
    padding: 14px 14px 12px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
  }

  .mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
    color: var(--color-muted);
  }

  .where {
    min-inline-size: 0;
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  h3 {
    margin: 8px 0 0;
    font-size: var(--text-page-title);
    font-weight: 600;
    line-height: 19px;
    letter-spacing: -0.005em;
    overflow-wrap: anywhere;
    text-wrap: pretty;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-block-start: 10px;
    min-inline-size: 0;
  }

  .note {
    margin: 10px 0 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 14px;
  }

  .body :global(p) {
    text-wrap: pretty;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
    margin-block-start: 14px;
    min-inline-size: 0;
  }

  footer.fill {
    justify-content: stretch;
  }

  @keyframes ask-in {
    from {
      opacity: 0;
      translate: 0 6px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .canvas {
      animation: none;
    }
  }
</style>

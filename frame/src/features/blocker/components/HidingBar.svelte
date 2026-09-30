<script lang="ts">
  import { hiding } from "$domain/blocker";
  import { tabs } from "$domain/tabs";

  let count = $derived(hiding.added().length);

  // Leaving the page ends hiding; the picker belongs to that document.
  $effect(() => {
    void tabs.activeId();
    void tabs.activeTab()?.url;
    return () => hiding.finish();
  });

  function onkeydown(event: KeyboardEvent) {
    if (!hiding.isActive()) return;
    if (event.key === "Escape") {
      event.preventDefault();
      hiding.finish();
    } else if (event.key === "z" && (event.metaKey || event.ctrlKey) && !event.shiftKey) {
      event.preventDefault();
      void hiding.undo();
    }
  }
</script>

<svelte:window {onkeydown} />

{#if hiding.isActive()}
  <div class="hiding" role="status" aria-live="polite">
    <span class="pulse" aria-hidden="true"></span>
    <span class="text">
      <span class="title">Hiding elements</span>
      <span class="detail"
        >{hiding.isSaving()
          ? "Saving…"
          : count === 0
            ? "Click anything on the page"
            : `${count} hidden on this site`}</span
      >
    </span>
    <button
      type="button"
      class="undo"
      disabled={count === 0 || hiding.isSaving()}
      title="Undo (⌘Z)"
      onclick={() => void hiding.undo()}>Undo</button
    >
    <button type="button" class="done" onclick={() => hiding.finish()}>Done</button>
  </div>
{/if}

<style>
  .hiding {
    display: flex;
    flex: none;
    align-items: center;
    gap: 10px;
    margin: 2px 8px 6px;
    padding: 8px 8px 8px 10px;
    border-radius: var(--radius-row);
    background: var(--row-active);
    box-shadow: inset 0 0 0 0.5px var(--color-border);
    animation: rise var(--motion-base) var(--ease-out) both;
  }

  /* A live dot, not an icon: hiding is a mode that is on, not a button. */
  .pulse {
    flex: none;
    width: 8px;
    height: 8px;
    margin-inline: 3px;
    border-radius: var(--radius-capsule);
    background: rgb(64 132 255);
    box-shadow: 0 0 0 3px rgb(64 132 255 / 22%);
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .title {
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 18px;
  }

  .detail {
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 16px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  button {
    flex: none;
    height: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-inset);
    font: inherit;
    font-size: var(--text-caption);
    font-weight: 500;
    cursor: default;
  }

  .undo {
    background: transparent;
    color: var(--color-text);
  }

  .undo:disabled {
    color: var(--color-faint);
  }

  .undo:hover:not(:disabled) {
    background: var(--row-hover);
  }

  .done {
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  .done:hover {
    background: var(--color-lit-hover);
  }

  @keyframes rise {
    from {
      opacity: 0;
      translate: 0 -4px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .hiding {
      animation: none;
    }
  }
</style>

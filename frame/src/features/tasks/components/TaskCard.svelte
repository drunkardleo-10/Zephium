<script lang="ts">
  import * as m from "$shared/i18n/messages";
  let {
    title,
    completed,
    dueDate,
    pinned,
    selected = false,
    onopen,
    ontoggle,
    disabled = false,
  }: {
    title: string;
    completed: boolean;
    dueDate: string | null;
    pinned: boolean;
    selected?: boolean;
    onopen: () => void;
    ontoggle?: (checked: boolean) => void;
    disabled?: boolean;
  } = $props();
</script>

<div class="task-card" class:selected>
  {#if ontoggle}<button
      type="button"
      class="toggle"
      role="checkbox"
      aria-checked={completed}
      aria-label={title}
      {disabled}
      onclick={() => ontoggle?.(!completed)}>{completed ? "✓" : "○"}</button
    >{:else}<span
      class="readonly-status"
      aria-label={completed ? m.work_item_complete() : m.work_item_open()}
      >{completed ? "✓" : "○"}</span
    >{/if}<button type="button" class="open" onclick={onopen}
    ><span class="body"
      ><strong>{title}</strong>{#if dueDate}<small>{dueDate}</small>{/if}</span
    >{#if pinned}<span aria-label={m.resource_pinned()}>•</span>{/if}</button
  >
</div>

<style>
  .readonly-status {
    padding: 6px;
    color: var(--color-muted);
  }

  .task-card {
    display: flex;
    align-items: center;
    border: 1px solid transparent;
    border-radius: var(--radius-md);
  }

  .task-card.selected {
    background: var(--color-fill-active);
    border-color: var(--color-border);
  }

  button.toggle {
    width: 32px;
    flex-shrink: 0;
    padding: 6px;
  }

  button.open {
    flex: 1;
    min-width: 0;
  }

  button {
    display: flex;
    gap: 10px;
    align-items: center;
    width: 100%;
    text-align: start;
    padding: 12px;
    font: inherit;
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-text);
    cursor: pointer;
  }

  .body {
    display: grid;
    gap: 4px;
    flex: 1;
    min-width: 0;
  }

  strong {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  small {
    color: var(--color-muted);
  }

  button:hover {
    background: var(--color-fill-hover);
  }

  button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>

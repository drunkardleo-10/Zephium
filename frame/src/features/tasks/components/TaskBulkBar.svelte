<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import {
    Cancel01Icon,
    Delete02Icon,
    Tick02Icon,
    ArrowTurnBackwardIcon,
  } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";

  let {
    count,
    trashed = false,
    onclear,
    oncomplete,
    onremove,
  }: {
    count: number;
    trashed?: boolean;
    onclear: () => void;
    oncomplete: () => void;
    onremove: () => void;
  } = $props();
</script>

<!--
  Sticks to the top of the list rather than replacing its header, so the rows a
  selection refers to stay in view while it is acted on.
-->
<div class="bulk" role="toolbar" aria-label={m.task_selection({ count })}>
  <span class="bulk-count">{m.task_selection({ count })}</span>
  {#if !trashed}
    <button type="button" onclick={oncomplete}
      ><Icon icon={Tick02Icon} size={13} />{m.task_state_done()}</button
    >
  {/if}
  <button type="button" onclick={onremove}
    ><Icon icon={trashed ? ArrowTurnBackwardIcon : Delete02Icon} size={13} />{trashed
      ? m.task_restore()
      : m.task_delete()}</button
  >
  <button type="button" class="bulk-clear" aria-label={m.task_selection_clear()} onclick={onclear}
    ><Icon icon={Cancel01Icon} size={13} /></button
  >
</div>

<style>
  .bulk {
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
    min-width: 0;
    margin-block-end: 6px;
    padding: 6px 8px;
    border-radius: var(--radius-row);
    background: var(--color-raised);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
    font-size: var(--text-caption);
    animation: bulk-in var(--motion-fast) var(--ease-out);
  }

  @keyframes bulk-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }

  .bulk-count {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-muted);
    font-variant-numeric: tabular-nums;
  }

  button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: none;
    height: 22px;
    padding-inline: 7px;
    border: 0;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  button:hover {
    background: var(--color-fill-hover);
  }

  button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .bulk-clear {
    width: 22px;
    padding: 0;
    justify-content: center;
    background: transparent;
    color: var(--color-faint);
  }
</style>

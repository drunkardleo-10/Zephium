<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { Cancel01Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import type { TaskNotice } from "$domain/resources";

  let {
    notice,
    onundo,
    ondismiss,
  }: {
    notice: TaskNotice | null;
    onundo: () => void;
    ondismiss: () => void;
  } = $props();

  /** Long enough to reach for, short enough not to linger over the next task. */
  const VISIBLE_MS = 5000;
  const VERB = {
    complete: m.task_notice_completed,
    delete: m.task_notice_deleted,
    restore: m.task_notice_restored,
    change: m.task_notice_completed,
  } as const;

  let hovered = $state(false);
  $effect(() => {
    if (!notice || hovered) return;
    const timer = setTimeout(ondismiss, VISIBLE_MS);
    return () => clearTimeout(timer);
  });
</script>

<!--
  Visual only: the list already announces each change to assistive technology,
  and Cmd+Z reaches the same undo from the keyboard.
-->
{#if notice}
  {#key notice.serial}
    <div
      class="notice"
      role="group"
      aria-label={VERB[notice.action]()}
      onpointerenter={() => (hovered = true)}
      onpointerleave={() => (hovered = false)}
    >
      <span class="notice-copy"
        ><strong>{VERB[notice.action]()}</strong><span>{notice.title}</span></span
      >
      <button type="button" class="notice-undo" onclick={onundo}>{m.task_undo()}</button>
      <button
        type="button"
        class="notice-close"
        aria-label={m.task_notice_dismiss()}
        onclick={ondismiss}><Icon icon={Cancel01Icon} size={13} /></button
      >
    </div>
  {/key}
{/if}

<style>
  .notice {
    position: absolute;
    inset-block-end: 16px;
    inset-inline-start: 50%;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 4px;
    max-width: min(420px, calc(100% - 24px));
    padding: 4px 4px 4px 14px;
    border-radius: var(--radius-control);
    background: var(--color-float);
    box-shadow: var(--shadow-float);
    color: var(--color-text);
    font-size: var(--text-body);
    translate: -50% 0;
    animation: notice-in var(--motion-base) var(--ease-out);
  }

  .notice:dir(rtl) {
    translate: 50% 0;
  }

  @keyframes notice-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  .notice-copy {
    display: flex;
    align-items: baseline;
    gap: 6px;
    flex: 1;
    min-width: 0;
    margin-inline-end: 8px;
  }

  .notice-copy strong {
    flex: none;
    font-weight: 600;
  }

  .notice-copy span {
    overflow: hidden;
    color: var(--color-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .notice-undo,
  .notice-close {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
    height: 28px;
    padding-inline: 10px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-weight: 550;
    cursor: default;
    outline: none;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  .notice-close {
    width: 28px;
    padding: 0;
    justify-content: center;
    color: var(--color-muted);
  }

  .notice-undo:hover,
  .notice-close:hover {
    background: var(--color-fill-hover);
  }

  .notice-undo:focus-visible,
  .notice-close:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .notice {
      animation: none;
    }
  }
</style>

<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { Cancel01Icon } from "@hugeicons/core-free-icons";
  import type { NoteSession } from "$domain/notes";
  import * as m from "$shared/i18n/messages";

  let { session }: { session: NoteSession } = $props();
  let notice = $derived(session.notice);
</script>

{#if notice}
  {#key notice.at}
    <div
      class="notice"
      role="status"
      onpointerenter={() => session.holdNotice(true)}
      onpointerleave={() => session.holdNotice(false)}
    >
      <span class="notice-copy">{m.note_moved({ title: notice.title || m.note_untitled() })}</span>
      <button type="button" class="notice-undo" onclick={() => void session.undo()}
        >{m.note_undo()}</button
      >
      <button
        type="button"
        class="notice-close"
        aria-label={m.note_dismiss()}
        onclick={() => session.dismissNotice()}><Icon icon={Cancel01Icon} size={13} /></button
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
    flex: 1;
    min-width: 0;
    margin-inline-end: 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .notice-undo,
  .notice-close {
    display: inline-flex;
    align-items: center;
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

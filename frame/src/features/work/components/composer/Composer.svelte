<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "$shared/ui/Icon";
  import { ArrowUp02Icon, Mic01Icon, StopIcon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    value = $bindable(""),
    placeholder,
    disabled = false,
    busy = false,
    canStop = false,
    context,
    above,
    footer,
    onsubmit,
    onstop,
    ref = $bindable(),
  }: {
    value?: string;
    placeholder: string;
    disabled?: boolean;
    busy?: boolean;
    canStop?: boolean;
    context?: Snippet;
    above?: Snippet;
    footer?: Snippet;
    onsubmit: () => void;
    onstop?: () => void;
    ref?: HTMLElement;
  } = $props();
  const id = $props.id();
  let textarea = $state<HTMLTextAreaElement>();
  function grow() {
    const element = textarea;
    if (!element) return;
    element.style.blockSize = "auto";
    element.style.blockSize = `${Math.min(element.scrollHeight, 160)}px`;
  }
  $effect(() => {
    void value;
    grow();
  });
  function keydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      if (value.trim() && !disabled) onsubmit();
    }
  }
</script>

<div class="composer" bind:this={ref}>
  {#if above}<div class="above">{@render above()}</div>{/if}
  <form
    class="panel"
    class:active={!!value.trim()}
    onsubmit={(event) => {
      event.preventDefault();
      if (value.trim() && !disabled) onsubmit();
    }}
  >
    {#if context}<div class="context">{@render context()}</div>{/if}
    <label class="sr-only" for={id}>{placeholder}</label>
    <div class="row">
      <textarea
        {id}
        bind:this={textarea}
        bind:value
        rows="1"
        {placeholder}
        maxlength="8192"
        {disabled}
        onkeydown={keydown}
        oninput={grow}></textarea>
      <div class="actions">
        <button
          type="button"
          class="icon voice"
          aria-label={m.work_env_voice_input()}
          title={m.work_env_voice_soon()}
          disabled
        >
          <Icon icon={Mic01Icon} size={17} />
        </button>
        {#if canStop}
          <button type="button" class="icon stop" aria-label={m.work_cancel()} onclick={onstop}>
            <Icon icon={StopIcon} size={16} />
          </button>
        {:else}
          <button
            type="submit"
            class="icon send"
            aria-label={m.work_env_send()}
            disabled={disabled || busy || !value.trim()}
          >
            <Icon icon={ArrowUp02Icon} size={17} strokeWidth={2} />
          </button>
        {/if}
      </div>
    </div>
    {#if footer}<div class="footer">{@render footer()}</div>{/if}
  </form>
</div>

<style>
  .composer {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
    pointer-events: none;
  }

  .above,
  .panel {
    pointer-events: auto;
  }

  .above {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-block-size: min(48vh, 480px);
    overflow: auto;
    padding-inline: 4px;
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: 6px;
    box-sizing: border-box;
    padding: 10px 10px 8px 14px;
    border-radius: var(--radius-panel) var(--radius-panel) 0 0;
    background: var(--color-menu);
    backdrop-filter: blur(14px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    transition: box-shadow var(--motion-base) var(--ease-smooth);
  }

  .panel:focus-within {
    box-shadow:
      var(--shadow-popover),
      0 0 0 1px var(--color-border-strong);
  }

  .context {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .row {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }

  textarea {
    flex: 1;
    min-inline-size: 0;
    box-sizing: border-box;
    min-block-size: 32px;
    max-block-size: 160px;
    padding: 6px 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: 14px;
    line-height: 20px;
    resize: none;
    outline: none;
  }

  textarea::placeholder {
    color: var(--color-faint);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .icon {
    display: grid;
    place-items: center;
    inline-size: 32px;
    block-size: 32px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--color-muted);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      color var(--motion-fast) var(--ease-smooth),
      opacity var(--motion-fast) var(--ease-smooth),
      scale var(--motion-base) var(--ease-spring);
  }

  .icon:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .icon:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .icon:active:not(:disabled) {
    scale: 0.94;
  }

  .voice:disabled {
    opacity: 0.55;
  }

  .send {
    background: var(--color-accent);
    color: var(--color-on-accent);
    box-shadow: var(--shadow-primary);
  }

  .send:disabled {
    background: var(--color-fill);
    color: var(--color-faint);
    box-shadow: none;
  }

  .send:hover:not(:disabled) {
    background: var(--color-accent-hover);
    color: var(--color-on-accent);
  }

  .stop {
    background: var(--color-fill-active);
    color: var(--color-text);
  }

  .footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-block-size: 16px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>

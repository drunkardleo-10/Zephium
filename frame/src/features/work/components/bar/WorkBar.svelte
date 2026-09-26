<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "$shared/ui/Icon";
  import { ArrowUp02Icon, Mic01Icon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";

  let {
    value = $bindable(""),
    placeholder,
    disabled = false,
    busy = false,
    running = false,
    holding = false,
    model = null,
    tools,
    attach,
    line,
    context,
    above,
    onsubmit,
    ref = $bindable(),
  }: {
    value?: string;
    placeholder: string;
    disabled?: boolean;
    busy?: boolean;
    /** A run is going: the bar is its line, with room to add to it. */
    running?: boolean;
    /** Something the field opened is still in use, so it stays open. */
    holding?: boolean;
    /** The model the runs here use, when a run has said. */
    model?: string | null;
    tools?: Snippet;
    attach?: Snippet;
    line?: Snippet;
    context?: Snippet;
    above?: Snippet;
    onsubmit: () => void;
    ref?: HTMLElement;
  } = $props();
  const id = $props.id();
  let textarea = $state<HTMLTextAreaElement>();
  let focused = $state(false);
  const engaged = $derived(focused || !!value.trim() || holding);
  const mode = $derived(running ? "running" : engaged ? "focus" : "rest");

  function grow() {
    const element = textarea;
    if (!element) return;
    element.style.blockSize = "auto";
    element.style.blockSize = `${Math.min(element.scrollHeight, 92)}px`;
  }
  $effect(() => {
    void value;
    grow();
  });
  function submit() {
    if (value.trim() && !disabled) onsubmit();
  }
  function keydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      submit();
    }
    if (event.key === "Escape" && !value.trim()) textarea?.blur();
  }
</script>

<div class="work-bar" data-mode={mode} data-engaged={engaged} bind:this={ref}>
  {#if above}<div class="above">{@render above()}</div>{/if}
  <div
    class="surface"
    role="presentation"
    onfocusin={() => (focused = true)}
    onfocusout={(event) => {
      const next = event.relatedTarget;
      if (!(next instanceof Node && event.currentTarget.contains(next))) focused = false;
    }}
  >
    {#if line}<div class="line-slot">{@render line()}</div>{/if}
    {#if tools}
      <div class="tools" inert={mode !== "rest"} aria-hidden={mode !== "rest"}>
        <div class="tools-inner" role="toolbar" aria-label={m.work_env_toolbar()}>
          {@render tools()}
          <span class="rule" aria-hidden="true"></span>
        </div>
      </div>
    {/if}
    <form
      class="field"
      class:active={!!value.trim()}
      onsubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      {#if context}<div class="context">{@render context()}</div>{/if}
      <div class="row">
        {#if attach}<div class="side lead" inert={!engaged}>
            <div class="side-inner">{@render attach()}</div>
          </div>{/if}
        <label class="sr-only" for={id}>{placeholder}</label>
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
        <div class="side trail" inert={!engaged}>
          <div class="side-inner">
            {#if model}<span class="model" title={m.work_bar_model()}>{model}</span>{/if}
            <span class="mic" aria-hidden="true"><Icon icon={Mic01Icon} size={16} /></span>
            <button
              type="submit"
              class="send"
              aria-label={m.work_env_send()}
              disabled={disabled || busy || !value.trim()}
            >
              <Icon icon={ArrowUp02Icon} size={16} strokeWidth={2} />
            </button>
          </div>
        </div>
      </div>
    </form>
  </div>
</div>

<style>
  /* One bar on the canvas's bottom edge: its tools and the ask field at rest,
     the field alone once it is in use, the agent's line while a run goes. */
  .work-bar {
    --bar-width: 600px;

    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
    box-sizing: border-box;
    inline-size: min(var(--bar-width), calc(100% - 48px));
    pointer-events: none;
    transition: inline-size var(--motion-slow) var(--ease-emphasized);
  }

  .work-bar[data-mode="focus"] {
    --bar-width: 720px;
  }

  .work-bar[data-mode="running"] {
    --bar-width: 680px;
  }

  .above,
  .surface {
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

  .surface {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    grid-template-areas:
      "line line"
      "tools field";
    align-items: end;
    gap: 4px 0;
    box-sizing: border-box;
    padding: 8px;
    border-radius: var(--radius-panel) var(--radius-panel) 0 0;
    background: var(--color-menu);
    backdrop-filter: blur(14px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    transition: grid-template-columns var(--motion-slow) var(--ease-emphasized);
  }

  /* The line takes the row; the field keeps a narrow place beside it to add
     to the run, and widens when it is used. */
  .work-bar[data-mode="running"] .surface {
    grid-template-columns: minmax(0, 1fr) 200px;
    grid-template-areas: "line field";
    column-gap: 8px;
  }

  .work-bar[data-mode="running"][data-engaged="true"] .surface {
    grid-template-columns: minmax(0, 1fr) 380px;
  }

  .line-slot {
    grid-area: line;
    min-inline-size: 0;
  }

  .work-bar:not([data-mode="running"]) .line-slot {
    padding-block-end: 4px;
    border-block-end: 1px solid var(--color-border);
    margin-block-end: 4px;
  }

  .tools {
    grid-area: tools;
    display: grid;
    grid-template-columns: 1fr;
    align-self: center;
    opacity: 1;
    transition:
      grid-template-columns var(--motion-slow) var(--ease-emphasized),
      opacity var(--motion-fast) var(--ease-out);
  }

  .work-bar[data-mode="focus"] .tools {
    grid-template-columns: 0fr;
    opacity: 0;
    transition-timing-function: var(--ease-exit), var(--ease-exit);
  }

  .work-bar[data-mode="running"] .tools {
    display: none;
  }

  .tools-inner {
    display: flex;
    align-items: center;
    gap: 2px;
    min-inline-size: 0;
    overflow: hidden;
  }

  .rule {
    flex: none;
    inline-size: 1px;
    block-size: 20px;
    margin-inline: 6px 8px;
    background: var(--color-border);
  }

  .field {
    grid-area: field;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-inline-size: 0;
    margin: 0;
    padding: 3px 4px 3px 12px;
    border-radius: var(--radius-control);
    background: var(--color-field);
    transition:
      padding var(--motion-base) var(--ease-out),
      box-shadow var(--motion-base) var(--ease-out);
  }

  .field:focus-within {
    box-shadow: var(--shadow-field-focus);
  }

  .work-bar[data-engaged="true"] .field {
    padding-inline-start: 4px;
  }

  .context {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 5px 4px 0;
  }

  .row {
    display: flex;
    align-items: flex-end;
    min-inline-size: 0;
  }

  /* What the field holds only while it is in use: it opens from nothing
     beside the words, and closes back into them. */
  .side {
    display: grid;
    grid-template-columns: 0fr;
    flex: none;
    opacity: 0;
    transition:
      grid-template-columns var(--motion-slow) var(--ease-emphasized),
      opacity var(--motion-fast) var(--ease-out);
  }

  .work-bar[data-engaged="true"] .side {
    grid-template-columns: 1fr;
    opacity: 1;
  }

  .side-inner {
    display: flex;
    align-items: center;
    gap: 2px;
    min-inline-size: 0;
    overflow: hidden;
  }

  .lead .side-inner {
    padding-inline-end: 4px;
  }

  .trail .side-inner {
    padding-inline-start: 6px;
  }

  textarea {
    flex: 1;
    min-inline-size: 0;
    box-sizing: border-box;
    min-block-size: 30px;
    max-block-size: 92px;
    padding: 5px 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    line-height: 20px;
    resize: none;
    outline: none;
  }

  textarea::placeholder {
    color: var(--color-faint);
  }

  .model {
    max-inline-size: 140px;
    padding-inline: 8px;
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mic {
    display: grid;
    place-items: center;
    inline-size: 30px;
    block-size: 30px;
    color: var(--color-faint);
  }

  .send {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 30px;
    block-size: 30px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      scale var(--motion-base) var(--ease-spring);
  }

  .send:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .send:disabled {
    background: var(--color-fill);
    color: var(--color-faint);
  }

  .send:hover:not(:disabled) {
    background: var(--color-lit-hover);
  }

  .send:active:not(:disabled) {
    scale: 0.94;
  }

  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  @media (forced-colors: active) {
    .surface {
      border: 1px solid ButtonText;
      backdrop-filter: none;
    }
  }
</style>

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
    holding = false,
    model = null,
    tools,
    attach,
    context,
    above,
    trailing,
    onsubmit,
    ref = $bindable(),
  }: {
    value?: string;
    placeholder: string;
    disabled?: boolean;
    busy?: boolean;
    /** Something the field opened is still in use, so it stays open. */
    holding?: boolean;
    /** The model the runs here use, when a run has said. */
    model?: string | null;
    tools?: Snippet;
    attach?: Snippet;
    context?: Snippet;
    above?: Snippet;
    /** What stands before the mic, such as the model the runs use; it replaces the model's name. */
    trailing?: Snippet;
    onsubmit: () => void;
    ref?: HTMLElement;
  } = $props();
  const id = $props.id();
  let textarea = $state<HTMLTextAreaElement>();
  let focused = $state(false);
  let surface = $state<HTMLElement>();
  /** A menu or popover the field's own controls opened: it may live in a portal, but it is the bar's. */
  let menu = $state(false);
  const opened = (element: Element | undefined) =>
    !!element?.querySelector('.field [aria-expanded="true"]');
  $effect(() => {
    const element = surface;
    if (!element) return;
    const observer = new MutationObserver(() => {
      const open = opened(element);
      if (open === menu) return;
      menu = open;
      // A menu closed on something outside the bar: the bar lets go with it.
      if (!open)
        requestAnimationFrame(() => {
          if (!element.contains(document.activeElement)) focused = false;
        });
    });
    observer.observe(element, { subtree: true, attributeFilter: ["aria-expanded"] });
    return () => observer.disconnect();
  });
  const engaged = $derived(focused || menu || !!value.trim() || holding);
  /**
   * WebKit does not focus a button it clicks: a press inside the bar blurs
   * the field with nowhere named to go. The bar holds through its own
   * presses and lets go on a press outside it (a menu it opened aside).
   */
  let pressing = false;
  const mode = $derived(engaged ? "focus" : "rest");

  function grow() {
    const element = textarea;
    if (!element) return;
    element.style.blockSize = "auto";
    element.style.blockSize = `${Math.min(element.scrollHeight, 132)}px`;
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

<svelte:window
  onpointerdown={(event) => {
    const target = event.target instanceof Node ? event.target : null;
    if (!focused || menu || !surface || (target && surface.contains(target))) return;
    focused = false;
  }}
  onpointerup={() => requestAnimationFrame(() => (pressing = false))}
  onpointercancel={() => (pressing = false)}
/>

<div class="work-bar" data-mode={mode} data-engaged={engaged} bind:this={ref}>
  {#if above}<div class="above">{@render above()}</div>{/if}
  <div
    class="surface"
    role="presentation"
    bind:this={surface}
    onpointerdown={(event) => {
      if (event.target instanceof Element && event.target.closest(".field")) pressing = true;
    }}
    onfocusin={(event) => {
      // A tool's own panel is the tools at work, not the field: the bar stays as it is.
      if (event.target instanceof Element && event.target.closest(".tools")) return;
      focused = true;
    }}
    onfocusout={(event) => {
      const next = event.relatedTarget;
      if (next instanceof Node && event.currentTarget.contains(next)) return;
      if (pressing && !next) return;
      // A menu the field's own control opened holds the bar open while it is open, portals included.
      if (opened(event.currentTarget)) return;
      focused = false;
    }}
  >
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
      onsubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <div class="row">
        {#if attach}<div class="side lead" inert={!engaged}>
            <div class="side-inner">{@render attach()}</div>
          </div>{/if}
        {#if context && engaged}<div class="context">{@render context()}</div>{/if}
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
        <div class="trail">
          {#if trailing && engaged}{@render trailing()}
          {:else if model && engaged}<span class="model" title={m.work_bar_model()}>{model}</span
            >{/if}
          {#if value.trim()}<button
              type="submit"
              class="send"
              aria-label={m.work_env_send()}
              disabled={disabled || busy}
            >
              <Icon icon={ArrowUp02Icon} size={16} strokeWidth={2.2} />
            </button>{:else}<span class="mic" aria-hidden="true"
              ><Icon icon={Mic01Icon} size={17} strokeWidth={1.6} /></span
            >{/if}
        </div>
      </div>
    </form>
  </div>
</div>

<style>
  /* One thin sheet on the canvas's bottom edge. At rest it holds the canvas's
     own tools and the ask; in use it is the ask alone. The words sit on the
     sheet itself: there is no field inside the bar, the bar is the field. The
     run's line is not here; it stands at the canvas's top. */
  .work-bar {
    --bar-width: 620px;

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
    --bar-width: 700px;
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
    grid-template-areas: "tools field";
    align-items: end;
    gap: 4px 0;
    box-sizing: border-box;
    padding: 8px 8px 10px;
    border-radius: var(--radius-panel) var(--radius-panel) 0 0;

    /* Solid, like the island: a blur would be redrawn from the canvas under it every frame a pan moves. */
    background: color-mix(in srgb, var(--color-float) var(--wash-raised), var(--color-canvas));
    box-shadow: var(--shadow-sheet);
    transition: grid-template-columns var(--motion-slow) var(--ease-emphasized);
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

  .tools-inner {
    display: flex;
    align-items: center;
    gap: 0;
    min-inline-size: 0;
    overflow: hidden;
  }

  /* At rest nothing folds, so the tools' hints may rise above the bar. */
  .work-bar[data-mode="rest"] .tools-inner {
    overflow: visible;
  }

  .rule {
    flex: none;
    inline-size: 1px;
    block-size: 18px;
    margin-inline: 6px 4px;
    background: var(--color-border);
  }

  .field {
    grid-area: field;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-inline-size: 0;
    margin: 0;
    padding: 0 0 0 8px;
  }

  .work-bar[data-engaged="true"] .field {
    padding-inline-start: 0;
  }

  /* What goes with the ask, as chips at the head of the field. */
  .context {
    display: flex;
    flex: none;
    align-items: center;
    align-self: center;
    gap: 6px;
  }

  .row {
    display: flex;
    align-items: flex-end;
    gap: 4px;
    min-inline-size: 0;
  }

  /* The attach button opens from nothing beside the words once the tools
     have folded away, and closes back into them. */
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
    min-inline-size: 0;
    overflow: hidden;
  }

  .trail {
    display: flex;
    flex: none;
    align-items: center;
    gap: 2px;
  }

  textarea {
    flex: 1;
    min-inline-size: 0;
    box-sizing: border-box;
    min-block-size: 34px;
    max-block-size: 132px;
    padding: 7px 4px;
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

  .model {
    max-inline-size: 140px;
    padding-inline: 8px;
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 34px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mic {
    display: grid;
    place-items: center;
    inline-size: 34px;
    block-size: 34px;
    color: var(--color-muted);
  }

  .send {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 30px;
    block-size: 30px;
    margin: 2px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
    cursor: default;
    animation: send-in var(--motion-base) var(--ease-spring);
    transition:
      background-color var(--motion-fast) var(--ease-out),
      scale var(--motion-base) var(--ease-spring);
  }

  @keyframes send-in {
    from {
      opacity: 0;
      scale: 0.6;
    }
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
    scale: 0.92;
  }

  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  @media (prefers-reduced-motion: reduce) {
    .send {
      animation: none;
    }
  }

  @media (forced-colors: active) {
    .surface {
      border: 1px solid ButtonText;
      backdrop-filter: none;
    }
  }
</style>

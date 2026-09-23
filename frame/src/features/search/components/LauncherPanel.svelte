<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { Search01Icon, Cancel01Icon, ArrowRight01Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import type { PanelState, ToolKind } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import ResultList from "./ResultList.svelte";
  import { createSearchSurface, type Destination } from "../lib/search-surface.svelte";

  let {
    context = null,
    onTool = () => {},
    onDrag = () => {},
    destinations = [],
  }: {
    context?: PanelState | null;
    onTool?: (tool: ToolKind) => void;
    onDrag?: () => void;
    destinations?: Destination[];
  } = $props();

  // Keyed on the panel session by its host, so the bound context is captured
  // once rather than tracked into a live request.
  const surface = createSearchSurface({
    tabId: null,
    context: untrack(() => context),
    destinations: untrack(() => destinations),
    onTool: (tool) => onTool(tool),
  });

  let input = $state<HTMLInputElement>();
  let list = $state<HTMLElement>();

  $effect(() => {
    surface.setList(list);
  });
  $effect(() => {
    void surface.query;
    surface.applyCompletion(input);
  });

  function keydown(event: KeyboardEvent) {
    if (surface.composing || event.isComposing) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      void surface.move(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      surface.submit();
    }
  }

  onMount(() => {
    input?.focus();
    return surface.mount();
  });
</script>

<div class="launcher">
  <header
    role="group"
    aria-label={m.panel_search_mode()}
    class="dragbar"
    onpointerdown={(event) => {
      if (event.button === 0 && !(event.target as HTMLElement).closest("button,input,a")) onDrag();
    }}
  >
    <span class="context">{context?.profile_name ?? m.panel_context_unavailable()}</span><span
      class="grip"
      aria-hidden="true"
    ></span><span class="context">{m.panel_search_mode()}</span>
  </header>

  <div class="field">
    <Icon icon={Search01Icon} size={20} /><input
      bind:this={input}
      value={surface.query}
      oninput={(event) => surface.changed(event.currentTarget.value)}
      oncompositionstart={surface.compositionStart}
      oncompositionend={(event) => surface.compositionEnd(event.currentTarget.value)}
      onkeydown={keydown}
      maxlength={2048}
      autocomplete="off"
      autocapitalize="off"
      spellcheck={false}
      placeholder={m.panel_search_placeholder()}
      aria-label={m.panel_search_placeholder()}
      role="combobox"
      aria-controls="launcher-results"
      aria-expanded={surface.rows.length > 0}
      aria-autocomplete="both"
      aria-activedescendant={surface.selectedIndex >= 0
        ? `launcher-results-option-${surface.selectedIndex}`
        : undefined}
    />{#if surface.query}<IconButton
        icon={Cancel01Icon}
        label={m.panel_clear_search()}
        onclick={() => {
          surface.changed("");
          input?.focus();
        }}
      />{/if}
  </div>

  <div class="scroll">
    {#if !surface.query.trim()}
      <section class="tools" aria-label={m.panel_tools()}>
        <h2>{m.panel_tools()}</h2>
        <div class="tool-grid">
          {#each destinations as destination (destination.kind)}
            {@const kind = destination.kind}<button type="button" onclick={() => onTool(kind)}
              ><span><Icon icon={destination.icon} size={18} /></span>{destination.label}<Icon
                icon={ArrowRight01Icon}
                size={13}
              /></button
            >
          {/each}
        </div>
      </section>
    {/if}

    {#if surface.failed || context?.error || surface.error !== "none"}
      <div class="failure" role="alert">
        {surface.error === "too_long" ? m.panel_query_too_long() : m.panel_action_failed()}
        <button type="button" onclick={surface.retry}>{m.panel_retry()}</button>
      </div>
    {/if}

    <ResultList
      rows={surface.rows}
      selected={surface.selectedId}
      query={surface.answered}
      listId="launcher-results"
      busy={surface.pending}
      bind:ref={list}
      onhover={surface.hover}
      onrun={surface.activate}
    />

    {#if surface.empty}<p class="empty">{m.panel_no_results()}</p>{/if}
  </div>

  <footer>
    <span aria-live="polite">{surface.running ? m.panel_opening() : m.panel_search_hint()}</span
    ><span><kbd>↑↓</kbd> {m.panel_navigate()}</span><span><kbd>↵</kbd> {m.panel_open()}</span><span
      ><kbd>esc</kbd> {m.panel_close()}</span
    >
  </footer>
</div>

<style>
  .launcher {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .dragbar {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 28px;
    padding: 5px 20px 0;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
  }

  .context {
    font-size: 10px;
    color: var(--color-faint);
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .grip {
    width: 28px;
    height: 3px;
    border-radius: 2px;
    background: var(--color-border-strong);
  }

  .field {
    display: flex;
    align-items: center;
    gap: 12px;
    flex: none;
    padding: 9px 20px 16px;
    border-bottom: 1px solid var(--color-border);
    color: var(--color-faint);
  }

  .field input {
    min-width: 0;
    flex: 1;
    height: 32px;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font-size: 18px;
    letter-spacing: -0.02em;
    outline: none;
  }

  .field input::placeholder {
    color: var(--color-faint);
    opacity: 1;
  }

  .scroll {
    min-height: 0;
    flex: 1;
    overflow-y: auto;
    padding: 8px 10px 12px;
    overscroll-behavior: contain;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 14px;
    flex: none;
    height: 36px;
    padding: 0 18px;
    border-top: 1px solid var(--color-border);
    font-size: 10px;
    color: var(--color-faint);
  }

  footer > span {
    display: flex;
    gap: 5px;
    align-items: center;
  }

  footer > span:first-child {
    margin-inline-end: auto;
  }

  kbd {
    font-family: var(--font-sans);
    font-size: 10px;
    color: var(--color-faint);
  }

  .empty {
    text-align: center;
    padding: 26px;
    margin: 0;
    font-size: 13px;
    color: var(--color-faint);
  }

  .failure {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 10px;
    font-size: 12px;
    color: var(--color-danger);
  }

  .failure button {
    padding: 5px 10px;
    border: 0;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-text);
    cursor: default;
  }

  .tools {
    margin-bottom: 14px;
  }

  .tools h2 {
    font-size: 11px;
    font-weight: 550;
    letter-spacing: 0.02em;
    color: var(--color-faint);
    margin: 8px 10px 6px;
  }

  .tool-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 2px 12px;
  }

  .tool-grid button {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    height: 40px;
    text-align: start;
    padding: 6px 10px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    font-size: 13px;
    cursor: default;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  .tool-grid button > span {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
  }

  .tool-grid button > :global(svg:last-child) {
    margin-inline-start: auto;
    color: var(--color-faint);
  }

  .tool-grid button:hover {
    background: var(--color-fill-active);
  }

  @media (prefers-reduced-motion: reduce) {
    .tool-grid button {
      transition: none;
    }
  }
</style>

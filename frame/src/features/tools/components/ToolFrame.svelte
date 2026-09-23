<script lang="ts">
  import type { Snippet } from "svelte";
  import { onMount } from "svelte";
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import { tools } from "../components/tool-views";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import SegmentedControl from "$shared/ui/SegmentedControl";
  import {
    ArrowLeft02Icon,
    Cancel01Icon,
    Add01Icon,
    Search01Icon,
  } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import "./tools.css";
  let {
    tool,
    state,
    edit,
    host,
    onclose,
    onback,
    ondrag,
    searchLabel,
    searchOpen = true,
    searchFocus = true,
    onsearchdismiss,
    heading,
    filters,
    actions,
    canCompose = false,
    marked = true,
    closable = true,
    caption = true,
    scrolls = true,
    children,
    footer,
  }: ToolHostProps & {
    searchLabel?: string;
    /** False keeps the search field out of the frame until it is asked for, so
     *  a tool that rarely needs it does not spend a band on it. */
    searchOpen?: boolean;
    /** False where the field is always shown, so opening the tool does not pull
     *  focus away from wherever the reader already is. */
    searchFocus?: boolean;
    onsearchdismiss?: () => void;
    /** Replaces the tool's name, for a tool whose title is also a control. */
    heading?: Snippet;
    filters?: { value: string; label: string }[];
    /** Tool-specific header controls, placed before the shared ones. */
    actions?: Snippet;
    canCompose?: boolean;
    /** False for a tool whose title is its own control and needs no mark. */
    marked?: boolean;
    /** False where the tool offers closing among its own actions. */
    closable?: boolean;
    /** A finished tool does not describe itself as a layout study. */
    caption?: boolean;
    /** Set false when the view owns a scroller of its own, so the frame does
     *  not nest one inside another and swallow the child's height. */
    scrolls?: boolean;
    children?: Snippet;
    footer?: Snippet;
  } = $props();
  let meta = $derived(tools[tool]);
  let content: HTMLElement;
  let header: HTMLElement;
  // The field only exists while it is open, so mounting it is opening it, and
  // asking for it is asking to type in it. (`$state` is unavailable here: this
  // component already has a prop called `state`.)
  const takeFocus = (node: HTMLInputElement) => {
    if (searchFocus) node.focus();
  };
  onMount(() => {
    content.scrollTop = state.scrollTop;
    if (host === "floating") header.querySelector<HTMLButtonElement>("button")?.focus();
  });
  function drag(event: PointerEvent) {
    if (
      event.button === 0 &&
      !(event.target as HTMLElement).closest("button,input,textarea,select,a")
    )
      ondrag?.();
  }
</script>

<section class="shared-tool" data-host={host} data-tool={tool} aria-label={meta.title()}>
  <header
    bind:this={header}
    role="group"
    aria-label={meta.title()}
    class="shared-tool-header"
    onpointerdown={drag}
  >
    <!-- The launcher is closed with Escape or a click outside it, so it needs no
         close button, only the way back to its menu; the sidebar is the reverse. -->
    {#if host === "floating" && onback}<IconButton
        icon={ArrowLeft02Icon}
        label={m.panel_back()}
        onclick={onback}
      />{/if}
    {#if marked}<span class="shared-tool-icon"><Icon icon={meta.icon} size={16} /></span>{/if}
    {#if heading}{@render heading()}{:else}<h2>{meta.title()}</h2>{/if}
    {#if actions}{@render actions()}{/if}{#if canCompose}<IconButton
        icon={Add01Icon}
        label={tool === "notes" ? m.tool_add_note() : m.tool_add_task()}
        onclick={() => edit({ composing: !state.composing })}
      />{/if}{#if host !== "floating" && closable}<IconButton
        icon={Cancel01Icon}
        label={m.tool_close()}
        onclick={onclose}
      />{/if}
  </header>
  {#if (searchLabel && searchOpen) || filters}<div
      class="shared-tool-controls"
      data-tauri-drag-region="false"
    >
      {#if searchLabel && searchOpen}<div class="shared-tool-search">
          <Icon icon={Search01Icon} size={15} /><input
            use:takeFocus
            type="search"
            aria-label={searchLabel}
            placeholder={searchLabel}
            value={state.query}
            maxlength={1024}
            oninput={(event) => edit({ query: event.currentTarget.value })}
            onkeydown={(event) => {
              // An empty field lets Escape through to the host, which in the
              // launcher means going back.
              if (event.key !== "Escape" || !onsearchdismiss || !event.currentTarget.value) return;
              event.preventDefault();
              event.stopPropagation();
              onsearchdismiss();
            }}
          />
        </div>{/if}{#if filters}<SegmentedControl
          label={meta.title()}
          full
          value={state.filter}
          options={filters}
          onchange={(filter) => edit({ filter })}
        />{/if}
    </div>{/if}
  <div
    class="shared-tool-content"
    class:shared-tool-content-hosted={!scrolls}
    data-tauri-drag-region="false"
    bind:this={content}
    onscroll={() => edit({ scrollTop: content.scrollTop })}
  >
    {#if children}{@render children()}{:else}<div class="shared-tool-empty">
        <span><Icon icon={meta.icon} size={28} /></span>
        <h3>{meta.empty()}</h3>
        <p>{meta.description()}</p>
      </div>{/if}
  </div>
  {#if footer}{@render footer()}{/if}
  {#if caption}<footer class="shared-tool-caption">{m.tool_preview()}</footer>{/if}
</section>

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
    profileName,
    onclose,
    onback,
    ondrag,
    searchLabel,
    filters,
    canCompose = false,
    scrolls = true,
    children,
    footer,
  }: ToolHostProps & {
    searchLabel?: string;
    filters?: { value: string; label: string }[];
    canCompose?: boolean;
    /** Set false when the view owns a scroller of its own, so the frame does
     *  not nest one inside another and swallow the child's height. */
    scrolls?: boolean;
    children?: Snippet;
    footer?: Snippet;
  } = $props();
  let meta = $derived(tools[tool]);
  let content: HTMLElement;
  let header: HTMLElement;
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

<section class="shared-tool" data-host={host} aria-label={meta.title()}>
  <header
    bind:this={header}
    role="group"
    aria-label={meta.title()}
    class="shared-tool-header"
    onpointerdown={drag}
  >
    {#if onback}<IconButton
        icon={ArrowLeft02Icon}
        label={m.panel_back()}
        onclick={onback}
      />{:else}<Icon icon={meta.icon} size={16} />{/if}
    <h2>{meta.title()}</h2>
    {#if host === "floating" && profileName}<span class="tool-owner">{profileName}</span
      >{/if}{#if canCompose}<IconButton
        icon={Add01Icon}
        label={tool === "notes" ? m.tool_add_note() : m.tool_add_task()}
        onclick={() => edit({ composing: !state.composing })}
      />{/if}<IconButton icon={Cancel01Icon} label={m.tool_close()} onclick={onclose} />
  </header>
  {#if searchLabel || filters}<div class="shared-tool-controls">
      {#if searchLabel}<div class="shared-tool-search">
          <Icon icon={Search01Icon} size={15} /><input
            type="search"
            aria-label={searchLabel}
            placeholder={searchLabel}
            value={state.query}
            maxlength={1024}
            oninput={(event) => edit({ query: event.currentTarget.value })}
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
  <footer class="shared-tool-caption">{m.tool_preview()}</footer>
</section>

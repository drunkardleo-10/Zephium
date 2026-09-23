<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { Cancel01Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import SearchField from "$shared/ui/SearchField";
  import IconButton from "$shared/ui/IconButton";
  import ResultList from "./ResultList.svelte";
  import { createSearchSurface } from "../lib/search-surface.svelte";

  let { tabId }: { tabId: string | null } = $props();

  // The search session is bound to one blank tab for the life of this
  // component; Shell keys the surface on the active tab, so a change is a
  // remount, never a reassignment underneath a live session.
  const surface = createSearchSurface({
    tabId: untrack(() => tabId),
    context: null,
    destinations: [],
    onTool: () => {},
  });

  let input = $state<HTMLInputElement>();
  let root: HTMLDivElement;
  let list = $state<HTMLElement>();
  let content = $state<HTMLElement>();
  let contentHeight = $state(0);
  /** Height is only animated once a first measurement exists, so the list does
   *  not grow out of nothing when it opens. */
  let measured = $state(false);
  // The list is absolutely positioned and height-bounded by what is actually
  // below the field, so suggestions never move the New Tab layout.
  let available = $state(320);

  let visible = $derived(
    surface.open &&
      !!surface.query.trim() &&
      (surface.rows.length > 0 || surface.empty || surface.failed || surface.error !== "none"),
  );

  $effect(() => {
    surface.setList(list);
  });
  $effect(() => {
    // Reading the completion is what makes this run on each settled answer.
    void surface.query;
    surface.applyCompletion(input);
  });
  $effect(() => {
    const element = content;
    if (!element) {
      measured = false;
      return;
    }
    // Resizing the list to its content is what turns a result arriving into a
    // single settling movement rather than a jump.
    const observer = new ResizeObserver(([entry]) => {
      // Border box, because the padding is part of what the list must be tall
      // enough to show; a content-box measurement leaves a permanent scrollbar.
      contentHeight = entry!.borderBoxSize[0]!.blockSize;
      requestAnimationFrame(() => (measured = true));
    });
    observer.observe(element);
    return () => observer.disconnect();
  });

  function measure() {
    if (input)
      available = Math.max(
        120,
        Math.min(380, window.innerHeight - input.getBoundingClientRect().bottom - 28),
      );
  }

  function keydown(event: KeyboardEvent) {
    if (surface.composing || event.isComposing) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      void surface.move(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Escape") {
      if (surface.dismiss()) event.preventDefault();
    } else if (event.key === "Enter") {
      event.preventDefault();
      surface.submit();
    }
  }

  onMount(() => {
    input?.focus();
    measure();
    window.addEventListener("resize", measure);
    const stop = surface.mount();
    return () => {
      window.removeEventListener("resize", measure);
      stop();
    };
  });
</script>

<div
  bind:this={root}
  class="surface"
  style:--available={`${available}px`}
  onfocusout={(event) => {
    if (event.relatedTarget instanceof Node && root.contains(event.relatedTarget)) return;
    surface.blurred();
  }}
>
  <SearchField
    size="page"
    label={m.search_web()}
    placeholder={m.search_web()}
    value={surface.query}
    bind:ref={input}
    oninput={surface.changed}
    onsubmit={surface.submit}
    inputProps={{
      role: "combobox",
      "aria-controls": "newtab-search-results",
      "aria-expanded": visible,
      "aria-autocomplete": "both",
      "aria-activedescendant":
        visible && surface.selectedIndex >= 0
          ? `newtab-search-results-option-${surface.selectedIndex}`
          : undefined,
      maxlength: 2048,
      spellcheck: false,
      autocapitalize: "off",
      // Room for the clear control, which is placed over the capsule rather
      // than threaded through the shared field for one caller's sake.
      style: surface.query ? "padding-inline-end: 26px" : undefined,
      onkeydown: keydown,
      onfocus: () => {
        measure();
        surface.focused();
      },
      oncompositionstart: surface.compositionStart,
      oncompositionend: (event) => surface.compositionEnd(event.currentTarget.value),
    }}
  />
  {#if surface.query}
    <span class="clear">
      <IconButton
        icon={Cancel01Icon}
        label={m.panel_clear_search()}
        onclick={() => {
          surface.changed("");
          input?.focus();
        }}
      />
    </span>
  {/if}

  {#if visible}
    <div
      class="dropdown"
      data-measured={measured}
      style:block-size={contentHeight ? `min(${contentHeight}px, var(--available))` : undefined}
    >
      <div class="scroll">
        <div class="content" bind:this={content}>
          {#if surface.failed || surface.error !== "none"}
            <div class="failure" role="alert">
              {surface.error === "too_long" ? m.panel_query_too_long() : m.panel_action_failed()}
              <button type="button" onclick={surface.retry}>{m.panel_retry()}</button>
            </div>
          {/if}
          <ResultList
            rows={surface.rows}
            selected={surface.selectedId}
            query={surface.answered}
            listId="newtab-search-results"
            busy={surface.pending}
            bind:ref={list}
            onhover={surface.hover}
            onrun={surface.activate}
          />
          {#if surface.empty}<p class="empty">{m.panel_no_results()}</p>{/if}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .surface {
    position: relative;
    inline-size: 100%;
    text-align: start;
  }

  .clear {
    position: absolute;
    inset-block-start: 0;
    inset-inline-end: 12px;
    display: flex;
    align-items: center;
    block-size: 46px;
  }

  /* Sits just under the capsule and shares its family of curves, so the two
     read as one control rather than a list that happens to be nearby. */
  .dropdown {
    position: absolute;
    inset-block-start: calc(100% + 6px);
    inset-inline: 0;
    z-index: 1;
    max-block-size: var(--available);
    overflow: hidden;
    border-radius: var(--radius-card);
    background: var(--color-raised);
    box-shadow:
      inset 0 0 0 1px var(--color-border),
      var(--shadow-popover);
    animation: dropdown-in var(--motion-fast) var(--ease-out) both;
  }

  .dropdown[data-measured="true"] {
    transition: block-size var(--motion-fast) var(--ease-out);
  }

  @keyframes dropdown-in {
    from {
      opacity: 0;
      transform: translateY(-4px) scale(0.985);
    }
  }

  .scroll {
    max-block-size: var(--available);
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  /* Padding lives on the measured element so the animated height accounts for
     it. */
  .content {
    padding: 6px;
  }

  .empty {
    text-align: center;
    padding: 22px 12px;
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

  @media (prefers-reduced-motion: reduce) {
    .dropdown {
      animation: none;
    }

    .dropdown[data-measured="true"] {
      transition: none;
    }
  }

  @media (forced-colors: active) {
    .dropdown {
      border: 1px solid CanvasText;
    }
  }
</style>

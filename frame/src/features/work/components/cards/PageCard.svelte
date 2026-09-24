<script lang="ts">
  import { untrack } from "svelte";
  import AgentAvatar from "./AgentAvatar.svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { cardCountdown, reasonBadge, reasonSentence } from "../../lib/work-human";
  import * as m from "$shared/i18n/messages";
  let { item, selected, onhelp }: { item: CanvasItem; selected: boolean; onhelp?: () => void } =
    $props();
  /** Set while the run is holding this page open for a person. */
  const human = $derived(item.page?.human ?? null);
  /** Only a page still waiting asks for a person; the rest say so in the header. */
  const waiting = $derived(human?.phase === "waiting_for_human");
  const countdown = $derived(waiting && human ? cardCountdown(human.remaining) : null);
  // A frame that missed once must not pin the placeholder: the store serves it
  // again once the run settles, and every refresh mints a new generation.
  let failedFrame = $state<string | null>(null);
  const frame = $derived(item.page?.frame ?? null);
  const failed = $derived(frame !== null && failedFrame === frame);
  $effect(() => {
    const current = frame;
    untrack(() => {
      if (failedFrame !== null && failedFrame !== current) failedFrame = null;
    });
  });
</script>

<article class="page" class:selected class:live={item.page?.live} class:held={!!human}>
  <header class="work-drag-handle">
    <span class="host">{item.page?.host || item.kind}</span>
    {#if !item.unavailable}<span class="state">
        {item.status}
        <!-- The reader mark: the agent is on this page right now. -->
        {#if item.page?.live}<span class="reader"><AgentAvatar size={14} active /></span>{/if}
      </span>{/if}
  </header>
  <div class="frame">
    {#if frame && !failed}
      <img
        src={frame}
        alt={item.title}
        draggable="false"
        loading="eager"
        decoding="async"
        onerror={() => (failedFrame = frame)}
      />
    {:else}
      <span class="placeholder" aria-hidden="true">{(item.page?.host || "?").slice(0, 1)}</span>
    {/if}
    {#if waiting && human}
      <div class="needs">
        <p class="sentence">{reasonSentence(human.reason, item.page?.host ?? "")}</p>
        <p class="ask">
          <span class="why">{reasonBadge(human.reason)}</span>
          <span class="act">
            {#if countdown}<span class="left">{countdown}</span>{/if}
            {#if onhelp}<button
                type="button"
                class="help nodrag nopan"
                onclick={(event) => {
                  event.stopPropagation();
                  onhelp?.();
                }}>{m.work_human_help()}</button
              >{/if}
          </span>
        </p>
      </div>
    {/if}
  </div>
  <!-- A read that gave up says so in Rust's words, not with a shrug. -->
  {#if item.unavailable}<p class="reason" title={item.status}>{item.status}</p>{/if}
</article>

<style>
  .page {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    block-size: 100%;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-text);
    overflow: hidden;
    transition: box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .page:hover {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .page.selected {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      0 0 0 2px var(--color-accent-soft),
      0 0 0 3px var(--color-accent);
  }

  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__node.dragging) .page {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      var(--shadow-popover);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex: none;
    padding: 7px 10px 6px 12px;
    font-size: var(--text-label);
    line-height: 16px;
    cursor: grab;
  }

  .host {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }

  .state {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
    color: var(--color-muted);
    font-size: var(--text-caption);
    white-space: nowrap;
  }

  .reader {
    display: inline-flex;
  }

  .frame {
    position: relative;
    flex: 1;
    min-block-size: 0;
    margin: 0 8px 8px;
    border-radius: var(--radius-sm);
    background: var(--color-fill);
    overflow: hidden;
    display: grid;
    place-items: center;
  }

  .page.held .frame {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .needs {
    position: absolute;
    inset-inline: 0;
    inset-block-end: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 22px 8px 8px;
    background: linear-gradient(to top, var(--color-surface) 62%, transparent);
    font-size: var(--text-caption);
    line-height: 16px;
  }

  .sentence {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    margin: 0;
    overflow: hidden;
    color: var(--color-text);
    font-weight: 500;
    line-height: 14px;
    text-wrap: pretty;
  }

  .ask {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin: 0;
  }

  .why {
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
    color: var(--color-text);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .act {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 8px;
  }

  .left {
    color: var(--color-muted);
    font-variant-numeric: tabular-nums;
  }

  .help {
    block-size: 22px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-accent);
    color: var(--color-on-primary);
    font: inherit;
    font-size: var(--text-caption);
    font-weight: 600;
    cursor: default;
    transition: filter var(--motion-instant) ease;
  }

  .help:hover {
    filter: brightness(1.08);
  }

  .frame img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    object-position: top;
    pointer-events: none;
  }

  .reason {
    flex: none;
    margin: 0;
    padding: 0 12px 8px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .placeholder {
    font-size: 28px;
    font-weight: 700;
    color: var(--color-faint);
    text-transform: uppercase;
  }
</style>

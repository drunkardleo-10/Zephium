<script lang="ts">
  import { untrack } from "svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { cardCountdown, reasonBadge } from "../../lib/work-human";
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
        {#if item.page?.live}<span class="dot" aria-hidden="true"></span>{/if}{item.status}
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
      <p class="needs">
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
    {/if}
  </div>
  <!-- A read that gave up says so in Rust's words, not with a shrug. -->
  {#if item.unavailable}<p class="reason">{item.status}</p>{/if}
</article>

<style>
  .page {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    block-size: 100%;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    box-shadow: var(--shadow-popover);
    color: var(--color-text);
    overflow: hidden;
  }

  .page.selected {
    box-shadow:
      var(--shadow-popover),
      0 0 0 2px var(--color-accent-soft);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 12px;
    font-size: var(--text-label);
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
    color: var(--color-muted);
    white-space: nowrap;
  }

  .dot {
    inline-size: 6px;
    block-size: 6px;
    border-radius: 50%;
    background: var(--color-accent);
    animation: pulse 1.6s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
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
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin: 0;
    padding: 18px 8px 8px;
    background: linear-gradient(to top, var(--color-surface) 46%, transparent);
    font-size: var(--text-caption);
    line-height: 16px;
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
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    flex: none;
    margin: 0;
    padding: 0 12px 10px;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 17px;
    text-wrap: pretty;
    overflow: hidden;
  }

  .placeholder {
    font-size: 28px;
    font-weight: 700;
    color: var(--color-faint);
    text-transform: uppercase;
  }

  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }
</style>

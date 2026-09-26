<script lang="ts">
  import { untrack } from "svelte";
  import CardFrame from "./CardFrame.svelte";
  import HostGlyph from "./HostGlyph.svelte";
  import AccountBadge from "./AccountBadge.svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { cardCountdown, reasonBadge, reasonSentence } from "../../lib/work-human";
  import * as m from "$shared/i18n/messages";
  let { item, selected, onhelp }: { item: CanvasItem; selected: boolean; onhelp?: () => void } =
    $props();
  /** Set while the run is holding this page open for a person. */
  const human = $derived(item.page?.human ?? null);
  /** Only a page still waiting asks for a person; the rest say so under the host. */
  const waiting = $derived(human?.phase === "waiting_for_human");
  const countdown = $derived(waiting && human ? cardCountdown(human.remaining) : null);
  const url = $derived(item.page?.url ?? item.detail);
  const host = $derived(item.page?.host || item.title);
  /** Where on the site, until the read gives up or a person holds it: then why. */
  const path = $derived.by(() => {
    try {
      const parsed = new URL(url);
      return `${parsed.pathname}${parsed.search}`.replace(/^\/$/u, "");
    } catch {
      return "";
    }
  });
  const meta = $derived(item.unavailable || (human && !waiting) ? item.status : path);
  const account = $derived(item.page?.account ?? null);
  /** An open tab the request was shown, not a page it read: no frame to show. */
  const tab = $derived(!!item.page?.tab);
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

<CardFrame id={item.id} {selected} unavailable={item.unavailable} plain>
  <div class="page">
    <div class="frame" class:tab>
      {#if tab}
        <span class="shown">
          <span class="caption">{item.status}</span>
          <span class="title" title={item.title}>{item.title}</span>
        </span>
      {:else if frame && !failed}
        <img
          src={frame}
          alt={item.title}
          draggable="false"
          loading="eager"
          decoding="async"
          onerror={() => (failedFrame = frame)}
        />
      {:else}
        <span class="placeholder" aria-hidden="true"
          ><HostGlyph {host} {url} size={28} initial={false} /></span
        >
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
    <div class="about">
      <!-- The arc stands in for the mark while the agent reads this page. -->
      <span class="marks"
        ><HostGlyph
          {host}
          {url}
          loading={!!item.page?.live}
          initial={false}
        />{#if account}<AccountBadge host={account} />{/if}</span
      >
      <span class="titles">
        <strong class="host" title={url}>{host}</strong>
        {#if meta}<span class="meta" title={meta}>{meta}</span>{/if}
      </span>
    </div>
  </div>
</CardFrame>

<style>
  .page {
    display: flex;
    flex-direction: column;
    block-size: 100%;
    min-block-size: 0;
  }

  .frame {
    position: relative;
    display: grid;
    place-items: center;
    flex: 1;
    min-block-size: 0;
    overflow: hidden;
    border-block-end: 1px solid var(--color-border);
    background: var(--color-fill);
  }

  .frame img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    object-position: top;
    pointer-events: none;
  }

  .placeholder {
    display: grid;
    place-items: center;
  }

  /* A tab shown to the request: its title where a frame would be. */
  .frame.tab {
    place-items: stretch;
    background: var(--color-surface);
  }

  .shown {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-inline-size: 0;
    padding: 12px;
  }

  .caption {
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 13px;
  }

  .title {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 17px;
    text-wrap: pretty;
    overflow-wrap: anywhere;
  }

  .marks {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 4px;
  }

  .about {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
    min-inline-size: 0;
    padding: 8px 12px 12px;
  }

  .titles {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-inline-size: 0;
  }

  .host,
  .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .host {
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
  }

  .meta {
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 13px;
  }

  /* A person is needed: one band over the frame's foot, the reason on the lit rung. */
  .needs {
    position: absolute;
    inset-inline: 0;
    inset-block-end: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border-block-start: 1px solid var(--color-border);
    background: var(--color-surface);
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
    overflow: hidden;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-lit-soft);
    color: var(--color-text);
    font-weight: 600;
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
    block-size: 24px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
    font: inherit;
    font-size: var(--text-caption);
    font-weight: 600;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .help:hover {
    background: var(--color-lit-hover);
  }

  .help:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>

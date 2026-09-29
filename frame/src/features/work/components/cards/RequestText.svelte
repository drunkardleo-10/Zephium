<script lang="ts">
  import { getContext, untrack } from "svelte";
  import Icon from "$shared/ui/Icon";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { canvasBoard, type BoardActions } from "../../lib/canvas-context";
  import { RUN } from "../../lib/run/layout";
  import {
    CommandLineIcon,
    File01Icon,
    FileEditIcon,
    Folder01Icon,
    Search01Icon,
  } from "../../lib/icons";
  import Remembered from "../asks/Remembered.svelte";
  import * as m from "$shared/i18n/messages";

  let {
    item,
    selected,
    ontoggle,
    onaction,
  }: {
    item: CanvasItem;
    selected: boolean;
    ontoggle: () => void;
    /** The request's one action, when it has one, such as showing its plan. */
    onaction?: () => void;
  } = $props();

  let words = $state<HTMLElement>();
  let clipped = $state(false);
  $effect(() => {
    void item.title;
    void item.expanded;
    const element = words;
    if (!element) return;
    void item.turns;
    const cut = (node: Element) => node.scrollHeight > node.clientHeight + 1;
    const measure = () =>
      (clipped = cut(element) || [...(body?.querySelectorAll("dd") ?? [])].some(cut));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  });
  const LOCAL = {
    folder: Folder01Icon,
    file: File01Icon,
    search: Search01Icon,
    command: CommandLineIcon,
    change: FileEditIcon,
  } as const;
  const local = $derived(item.turns?.local ?? []);
  const exchange = $derived(item.turns?.exchange ?? []);
  // The run lays its request out at the height its words and turns really take.
  const board = getContext<BoardActions | undefined>(canvasBoard);
  let body = $state<HTMLElement>();
  $effect(() => {
    const element = body;
    const open = !!item.expanded;
    if (!element || !board) return;
    let reported = 0;
    const report = () => {
      const height = Math.max(80, Math.ceil(element.offsetHeight));
      if (height === reported) return;
      reported = height;
      untrack(() => board.measure(item.id, RUN.request, open, height));
    };
    report();
    const observer = new ResizeObserver(() => requestAnimationFrame(report));
    observer.observe(element);
    return () => observer.disconnect();
  });
</script>

<!--
  The person's words, as words: no card around them. Who and when above; two
  lines at rest, and a click opens every line where they stand.
-->
<div
  class="request work-drag-handle"
  class:selected
  data-work-request={item.id}
  data-card-id={item.id}
>
  <div class="body" bind:this={body}>
    <p class="meta">
      <span>{m.work_request_you()}</span>{#if item.when}<span class="dot" aria-hidden="true">·</span
        ><time>{item.when}</time>{/if}
    </p>
    <p class="words" class:open={item.expanded} bind:this={words}>{item.title}</p>
    {#if clipped || item.expanded || (item.actionLabel && onaction)}<p class="actions">
        {#if clipped || item.expanded}<button
            type="button"
            class="more nodrag nopan"
            aria-expanded={!!item.expanded}
            onclick={(event) => {
              event.stopPropagation();
              ontoggle();
            }}>{item.expanded ? m.work_request_less() : m.work_request_more()}</button
          >{/if}
        {#if item.actionLabel && onaction}<button
            type="button"
            class="more nodrag nopan"
            onclick={(event) => {
              event.stopPropagation();
              onaction();
            }}>{item.actionLabel}</button
          >{/if}
      </p>{/if}
    {#if local.length}<ul class="local">
        {#each local as line, index (index)}<li>
            <Icon icon={LOCAL[line.kind]} size={13} /><span>{line.text}</span>
          </li>{/each}
      </ul>{/if}
    {#if exchange.length}<!-- The run's questions and the person's answers: a conversation, read down. -->
      <dl class="exchange" class:open={item.expanded}>
        {#each exchange as turn, index (index)}
          {#if turn.kind === "ask"}<div class="turn">
              <dt>{m.work_turn_agent()}</dt>
              <dd class="asked">{turn.question}</dd>
            </div>
            <div class="turn">
              <dt>{m.work_request_you()}</dt>
              <dd class="said" class:pending={!turn.answer}>
                {turn.answer ?? m.work_turn_waiting()}
              </dd>
            </div>{:else}<div class="turn">
              <dt>{m.work_request_you()}</dt>
              <dd class="said">{turn.text}</dd>
            </div>{/if}
        {/each}
      </dl>{/if}
  </div>
  {#if item.remember}<div class="remembered">
      <Remembered {...item.remember} />
    </div>{/if}
</div>

<style>
  .request {
    position: relative;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    padding: 0 4px;
    border-radius: var(--radius-control);
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .request.selected {
    box-shadow: 0 0 0 1.5px var(--color-ring);
  }

  /* What the run remembered hangs under the words, clear of the thread. */
  .remembered {
    position: absolute;
    inset-block-start: calc(100% + 4px);
    inset-inline: 4px 0;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0 0 4px;
    color: var(--color-faint);
    font-size: var(--text-label);
    font-weight: 500;
    line-height: 16px;
  }

  .dot {
    opacity: 0.7;
  }

  time {
    font-variant-numeric: tabular-nums;
  }

  .words {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    color: var(--color-text);
    font-size: 17px;
    font-weight: 500;
    letter-spacing: -0.012em;
    line-height: 24px;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow-wrap: anywhere;
  }

  .words.open {
    display: block;
    overflow: visible;
    -webkit-line-clamp: unset;
    line-clamp: unset;
  }

  .actions {
    display: flex;
    gap: 14px;
    margin: 4px 0 0;
  }

  .more {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
  }

  .more:hover {
    color: var(--color-text);
  }

  .local {
    display: flex;
    flex-direction: column;
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
  }

  .local li {
    display: flex;
    align-items: center;
    gap: 7px;
    min-inline-size: 0;
    block-size: 20px;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .local li :global(svg) {
    flex: none;
    color: var(--color-faint);
  }

  .local span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* A speaker column and their words, like the lines of a play: no bubbles, no cards. */
  .exchange {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 12px 0 0;
    padding: 10px 0 0;
    border-block-start: 1px solid var(--color-border);
  }

  .turn {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr);
    align-items: baseline;
  }

  dt {
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.01em;
    line-height: 18px;
  }

  dd {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-body);
    line-height: 18px;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow-wrap: anywhere;
    text-wrap: pretty;
  }

  .exchange.open dd {
    display: block;
    -webkit-line-clamp: unset;
    line-clamp: unset;
  }

  .asked {
    color: var(--color-muted);
  }

  .said {
    color: var(--color-text);
    font-weight: 500;
  }

  .said.pending {
    color: var(--color-faint);
    font-weight: 400;
  }

  .more:focus-visible {
    border-radius: var(--radius-inset);
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>

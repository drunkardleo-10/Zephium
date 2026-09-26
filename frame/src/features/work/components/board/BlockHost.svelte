<script lang="ts">
  import { getContext, onMount, untrack } from "svelte";
  import { canvasBoard, type BoardActions } from "../../lib/canvas-context";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { PAD } from "../../lib/board/size";
  import Prose from "./Prose.svelte";
  import EntityCard from "./EntityCard.svelte";
  import Gallery from "./Gallery.svelte";
  import Diagram from "./Diagram.svelte";
  import Table from "./Table.svelte";
  import Chart from "./Chart.svelte";
  import Comparison from "./Comparison.svelte";
  import Checklist from "./Checklist.svelte";
  import Code from "./Code.svelte";
  import Document from "./Document.svelte";
  import Timeline from "./Timeline.svelte";
  import Stat from "./Stat.svelte";
  import Callout from "./Callout.svelte";
  import * as m from "$shared/i18n/messages";
  let {
    id,
    item,
    selected = false,
  }: { id: string; item: CanvasItem; selected?: boolean } = $props();
  const actions = getContext<BoardActions | undefined>(canvasBoard);
  const block = $derived(item.block!.data);
  const sources = $derived(item.block!.sources);
  const open = $derived(item.block!.open);
  const width = $derived(item.size?.width ?? 0);
  const inner = $derived(Math.max(0, width - PAD * 2));
  /** A block names itself by its title; prose and a lone entity say what they are by content. */
  const titled = $derived(!!block.title && block.kind !== "prose" && block.kind !== "entity");

  let root = $state<HTMLElement>();
  let body = $state<HTMLElement>();
  /** Far from the viewport a block is a light plate of its own size. */
  let near = $state(false);
  onMount(() => {
    const element = root;
    if (!element) return;
    const watch = new IntersectionObserver(
      (entries) => {
        const entry = entries.at(-1);
        if (entry) near = entry.isIntersecting;
      },
      { rootMargin: "100% 100%" },
    );
    watch.observe(element);
    return () => watch.disconnect();
  });
  // The block's own height at its width, so the board places what it holds, not a guess.
  let reported = "";
  $effect(() => {
    const element = body;
    const shown = near;
    const opened = open;
    const across = width;
    if (!element || !shown || block.state === "pending") return;
    const report = () => {
      const height = Math.ceil(element.offsetHeight);
      const key = `${across}|${opened}|${height}`;
      if (!height || key === reported) return;
      reported = key;
      untrack(() => actions?.measure(id, across, opened, height));
    };
    report();
    // Reported on the next frame, so the board's answer never lands inside this observation.
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(report);
    });
    observer.observe(element);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  });
</script>

<article
  bind:this={root}
  class="work-drag-handle block {block.kind}"
  class:selected
  class:open
  class:hero={block.emphasis === "hero"}
  class:pending={block.state === "pending"}
  data-card-id={id}
  aria-label={block.title || m.work_board_block()}
>
  <div class="body" bind:this={body}>
    {#if titled}<header>
        <h3>{block.title}</h3>
      </header>{/if}
    {#if !near}<div class="plate" aria-hidden="true">
        <span></span><span></span><span class="short"></span>
      </div>
    {:else if block.kind === "prose"}<Prose {block} {sources} onevidence={actions?.evidence} />
    {:else if block.kind === "entity"}<EntityCard entity={block.entity} {sources} {actions} wide />
    {:else if block.kind === "gallery"}<Gallery {block} {sources} {actions} {open} width={inner} />
    {:else if block.kind === "diagram"}<Diagram {block} width={inner} {actions} />
    {:else if block.kind === "table"}<Table {block} {actions} {open} />
    {:else if block.kind === "chart"}<Chart {block} width={inner} />
    {:else if block.kind === "comparison"}<Comparison {block} {actions} {open} />
    {:else if block.kind === "checklist"}<Checklist {block} />
    {:else if block.kind === "code"}<Code {block} {actions} {open} />
    {:else if block.kind === "document"}<Document {block} {actions} {open} />
    {:else if block.kind === "timeline"}<Timeline {block} width={inner} />
    {:else if block.kind === "stat"}<Stat {block} />
    {:else if block.kind === "callout"}<Callout {block} />{/if}
  </div>
</article>

<style>
  /* One surface for every block: the card rung, a hairline, room to read; it never clips
     what opens from it. */
  .block {
    box-sizing: border-box;
    block-size: 100%;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-text);
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .block:hover {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .block.selected {
    box-shadow:
      0 0 0 1px var(--color-lit),
      var(--shadow-raised);
  }

  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__node.dragging) .block {
    box-shadow:
      inset 0 0 0 1px var(--color-border),
      var(--shadow-float);
    cursor: grabbing;
  }

  .body {
    box-sizing: border-box;
    padding: 20px;
  }

  .prose .body {
    padding: 22px 24px 24px;
  }

  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    margin-block-end: 12px;
  }

  h3 {
    margin: 0;
    font-size: var(--text-page-title);
    font-weight: 600;
    line-height: 20px;
    letter-spacing: -0.005em;
  }

  .plate {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .plate span {
    block-size: 10px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
  }

  .plate .short {
    inline-size: 55%;
  }
</style>

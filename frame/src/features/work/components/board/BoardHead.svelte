<script lang="ts">
  import { getContext, untrack } from "svelte";
  import { provideSiteMarks } from "$shared/ui/data/Artifact/site-marks";
  import { siteMark } from "../cards/HostGlyph.svelte";
  import { canvasBoard, type BoardActions } from "../../lib/canvas-context";
  import type { CanvasItem } from "../../lib/canvas-model";
  import Prose from "./Prose.svelte";
  let { id, item }: { id: string; item: CanvasItem } = $props();
  const actions = getContext<BoardActions | undefined>(canvasBoard);
  // Source chips draw the sites' real marks.
  provideSiteMarks(siteMark);
  let body = $state<HTMLElement>();
  let reported = "";
  $effect(() => {
    const element = body;
    const width = item.size?.width ?? 0;
    if (!element) return;
    const report = () => {
      const height = Math.ceil(element.offsetHeight);
      const key = `${width}|${height}`;
      if (!height || key === reported) return;
      reported = key;
      untrack(() => actions?.measure(id, width, false, height));
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

<!-- The board's name and its answer in one sentence, set on the canvas itself. -->
<header class="head" bind:this={body}>
  {#if item.title}<h2>{item.title}</h2>{/if}
  {#if item.detail}<p>{item.detail}</p>{/if}
  {#if item.block?.data.kind === "prose"}<div class="more">
      <Prose block={item.block.data} sources={item.block.sources} onevidence={actions?.evidence} />
    </div>{/if}
</header>

<style>
  .head {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-inline-size: 72ch;
    pointer-events: none;
  }

  h2 {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-title);
    font-weight: 600;
    line-height: 28px;
    letter-spacing: -0.015em;
    text-wrap: balance;
  }

  .more {
    pointer-events: auto;
  }

  .more :global(.prose p) {
    color: var(--color-label-secondary);
  }

  p {
    margin: 0;
    color: var(--color-label-secondary);
    font-size: calc(var(--text-body) + 3px);
    line-height: 24px;
    text-wrap: pretty;
  }
</style>

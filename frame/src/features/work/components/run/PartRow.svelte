<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import type { CanvasItem, PartPage } from "../../lib/canvas-model";
  import type { Detail } from "../../lib/board/types";
  import { PART } from "../../lib/run/part-size";
  import { reasonBadge } from "../../lib/work-human";
  import {
    CommandLineIcon,
    ComputerTerminal01Icon,
    FileEditIcon,
    File01Icon,
    Plug01Icon,
    Search01Icon,
  } from "../../lib/icons";
  import { askCard, partContent } from "./slots";
  import { getContext, untrack } from "svelte";
  import { canvasBoard, canvasFocusResult, type BoardActions } from "../../lib/canvas-context";
  import { askKey } from "../../lib/run/part-size";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import PageFace from "./PageFace.svelte";
  import AgentOrb from "../cards/AgentOrb.svelte";
  import * as m from "$shared/i18n/messages";

  let {
    item,
    selected,
    detail = "full",
    onopen,
    onlist,
  }: {
    item: CanvasItem;
    selected: boolean;
    detail?: Detail;
    /** Opens one of the part's pages in the centre. */
    onopen: (page: string) => void;
    /** Opens everything the part read or cited. */
    onlist: () => void;
  } = $props();

  const part = $derived(item.part!);
  const shape = $derived(part.shape);
  /** The page it is on now leads, then the newest. */
  const ordered = $derived(
    [...part.pages].reverse().sort((a, b) => Number(b.live) - Number(a.live)),
  );
  const shown = $derived(ordered.slice(0, PART.shown));
  const hidden = $derived(Math.max(0, part.pages.length - PART.shown));
  const working = $derived(part.state === "running" || part.state === "waiting");
  const cited = $derived(part.cited ?? []);
  const more = $derived(Math.max(0, (part.citedCount ?? 0) - cited.length));

  /**
   * One place for each page, in frames or folded. Frames stand in a row; the
   * stack puts the newest in front, the two behind it up and to the right.
   */
  const SCALE = PART.thumb / PART.tile;
  function placeOf(index: number): string {
    if (shape === "frames") return `translate(${index * (PART.tile + PART.tileGap)}px, 0)`;
    return `translate(${index * PART.stackStep}px, ${index * 11}px) scale(${SCALE})`;
  }
  const lead = PART.label + PART.gap;
  /** Surveyed, a name is set as large as its column lets it stand without breaking a word. */
  const survey = $derived.by(() => {
    const longest = Math.max(...part.title.split(/\s+/u).map((word) => word.length), 1);
    return Math.round(Math.max(16, Math.min(26, 128 / (longest * 0.58))));
  });
  /** A helper's own view of its work, when its stream has built one. */
  const content = $derived(shape === "helper" ? partContent(part.helper) : null);
  const asking = $derived(part.ask ? askCard() : null);
  const LINE = {
    read: File01Icon,
    write: FileEditIcon,
    command: CommandLineIcon,
    search: Search01Icon,
  } as const;
  const lines = $derived(part.lines ?? []);
  const board = getContext<BoardActions | undefined>(canvasBoard);
  const focus = getContext<((id: string) => void) | undefined>(canvasFocusResult);
  // The ask card's own height, so its row makes the room it needs at each detail.
  let askBody = $state<HTMLElement>();
  $effect(() => {
    const element = askBody;
    const level = detail;
    if (!element) return;
    let reported = 0;
    const report = () => {
      const height = Math.ceil(element.offsetHeight);
      if (!height || height === reported) return;
      reported = height;
      untrack(() => board?.measure(askKey(item.id), PART.ask, false, height, level));
    };
    report();
    const observer = new ResizeObserver(() => requestAnimationFrame(report));
    observer.observe(element);
    return () => observer.disconnect();
  });
  /** A one-word name longer than its column is set smaller rather than cut. */
  const fitted = $derived.by(() => {
    const longest = Math.max(...part.title.split(/\s+/u).map((word) => word.length), 1);
    return Math.max(11, Math.min(13, Math.floor(97 / (longest * 0.56))));
  });
  /** Surveyed, a search part shows the sites it drew on, each once. */
  const sites = $derived([...new Map(cited.map((row) => [row.where, row])).values()]);
</script>

{#snippet face(page: PartPage)}
  <PageFace url={page.url} title={page.title} frame={page.frame} host={part.host ?? ""} />
  {#if page.human?.phase === "waiting_for_human"}<span class="needs"
      ><span class="why">{reasonBadge(page.human.reason)}</span><span
        class="help"
        role="presentation">{m.work_human_help()}</span
      ></span
    >{/if}
{/snippet}

<section
  class="part work-drag-handle {shape} {detail}"
  class:label-only={shape === "label"}
  class:selected
  class:working
  aria-label={part.title}
  data-part={item.id}
>
  <button
    type="button"
    class="label nodrag"
    onclick={(event) => {
      event.stopPropagation();
      onlist();
    }}
  >
    <span class="name" style:--survey="{survey}px" style:--fitted="{fitted}px">
      {#if part.helper === "research"}<span class="glyph"
          ><Icon icon={Search01Icon} size={14} /></span
        >{:else if part.helper === "computer"}<span class="glyph"
          ><Icon icon={ComputerTerminal01Icon} size={14} /></span
        >{:else if part.helper === "connection" && !part.host}<span class="glyph"
          ><Icon icon={Plug01Icon} size={14} /></span
        >{:else}<span class="mark"
          ><HostGlyph host={part.host ?? ""} size={16} loading={working} initial={false} /></span
        >{/if}
      <strong>{part.title}</strong>
    </span>
    {#if part.presence !== undefined}<span class="presence" aria-hidden="true"
        ><AgentOrb seed={part.presence} size={14} ring /></span
      >{/if}
    {#if part.summary}<span class="summary" class:turn={part.state === "waiting"}
        >{part.summary}</span
      >{/if}
  </button>

  {#if shape === "ask" && part.ask}
    <div class="slot ask" style:inset-inline-start="{lead}px" data-part-ask={item.id}>
      <div bind:this={askBody}>
        {#if asking}{#await asking() then view}<view.default
              {...part.ask.props}
              {detail}
              onfocus={() => focus?.(item.id)}
            />{/await}{/if}
      </div>
    </div>
  {:else if shape === "helper"}
    <div class="slot" style:inset-inline-start="{lead}px">
      {#if content}{#await content() then view}<view.default
            id={item.id}
            {part}
            {detail}
            objective={part.objective ?? ""}
            steps={part.steps ?? []}
          />{/await}
      {:else}<ul class="lines">
          {#each lines.slice(0, PART.helperLines) as line, index (index)}<li class={line.kind}>
              <Icon icon={LINE[line.kind]} size={13} /><span>{line.text}</span>
            </li>{/each}
          {#if lines.length > PART.helperLines}<li class="rest">
              {m.work_part_lines_more({ count: lines.length - PART.helperLines })}
            </li>{/if}
        </ul>{/if}
    </div>
  {:else if shape === "frames" || shape === "stack"}
    <div class="pages" style:inset-inline-start="{lead}px" style:--tile="{PART.tile}px">
      {#each shown as page, index (page.id)}
        <button
          type="button"
          class="page nodrag"
          class:live={page.live}
          style:--place={placeOf(index)}
          style:--fanned="translate({index * (PART.thumb + 10)}px, 0) scale({SCALE})"
          style:z-index={shown.length - index}
          title={page.title}
          aria-label={page.title}
          onclick={(event) => {
            event.stopPropagation();
            onopen(page.id);
          }}
        >
          <span class="glass">{@render face(page)}</span>
          <span class="caption">{page.tab ? page.status : page.title}</span>
        </button>
      {/each}
      {#if shape === "stack" && part.pages.length > 1}<span class="count">{part.pages.length}</span
        >{:else if hidden}<span
          class="more"
          style:inset-inline-start="{(shown.length - 1) * (PART.tile + PART.tileGap) +
            PART.tile -
            8}px">+{hidden}</span
        >{/if}
    </div>
  {:else if shape === "sources"}
    <ul class="cited" style:inset-inline-start="{lead}px">
      {#each detail === "full" ? cited : sites as row (row.key)}<li>
          <button
            type="button"
            class="nodrag"
            title={row.title}
            onclick={(event) => {
              event.stopPropagation();
              onlist();
            }}
          >
            <HostGlyph host={row.where} url={row.url} size={14} initial={false} />
            <span class="site">{row.where}</span>
            <span class="title">{row.title}</span>
          </button>
        </li>{/each}
      {#if more}<li class="rest">{m.work_part_more_cited({ count: more })}</li>{/if}
    </ul>
  {/if}
</section>

<style>
  .part {
    position: relative;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    border-radius: var(--radius-row);
    color: var(--color-text);
  }

  .label {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    box-sizing: border-box;
    inline-size: 120px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .label:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .name {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    min-inline-size: 0;
    padding-block: 3px;
  }

  .mark,
  .glyph {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 16px;
    block-size: 18px;
  }

  .glyph {
    color: var(--color-muted);
  }

  strong {
    display: -webkit-box;
    overflow: hidden;
    font-size: var(--fitted, var(--text-body));
    font-weight: 600;
    line-height: 18px;
    letter-spacing: -0.005em;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  /* Another helper at work: its own small orb at the corner of its row's name. */
  .presence {
    position: absolute;
    inset-block-start: -6px;
    inset-inline-start: -8px;
    animation: presence-in var(--motion-base) var(--ease-spring);
  }

  @keyframes presence-in {
    from {
      scale: 0.4;
      opacity: 0;
    }
  }

  .summary {
    padding-inline-start: 23px;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .summary.turn {
    color: var(--color-text);
    font-weight: 600;
  }

  .pages {
    position: absolute;
    inset-block-start: 0;
    block-size: 100%;
    inline-size: calc(100% - 136px);
  }

  .page {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    inline-size: var(--tile);
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
    transform: var(--place);
    transform-origin: 0 0;
    transition: transform var(--motion-slow) var(--ease-emphasized);
  }

  .glass {
    position: relative;
    display: block;
    aspect-ratio: 16 / 10;
    overflow: hidden;
    border-radius: var(--radius-control);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
    translate: 0 0;
    transition:
      box-shadow var(--motion-base) var(--ease-out),
      translate var(--motion-base) var(--ease-spring);
  }

  .page.live .glass {
    box-shadow:
      0 0 0 1.5px var(--color-accent),
      var(--shadow-raised);
  }

  .page:focus-visible .glass {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .caption {
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .stack .caption {
    opacity: 0;
  }

  /* Folded: hovering the stack fans its pages out along the row, over what follows. */
  .stack .pages:hover .page {
    transform: var(--fanned);
  }

  .count,
  .more {
    position: absolute;
    z-index: 4;
    padding: 1px 7px;
    border-radius: var(--radius-capsule);
    background: var(--color-float);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    line-height: 16px;
    pointer-events: none;
  }

  .count {
    inset-block-start: 0;
    inset-inline-start: 144px;
    translate: -50% -50%;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .more {
    inset-block-start: 6px;
  }

  .needs {
    position: absolute;
    inset: auto 6px 6px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 4px 4px 4px 8px;
    border-radius: var(--radius-control-compact);
    background: var(--color-float);
    box-shadow: var(--shadow-menu);
    color: var(--color-text);
    font-size: var(--text-caption);
    font-weight: 600;
    line-height: 14px;
  }

  .help {
    padding: 3px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  .slot {
    position: absolute;
    inset-block: 0;
    inset-inline-end: 0;
  }

  .lines {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .lines li {
    display: flex;
    align-items: center;
    gap: 8px;
    block-size: 24px;
    min-inline-size: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .lines span {
    overflow: hidden;
    color: var(--color-text);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .lines .command span {
    font-family: var(--font-mono);
  }

  .lines .rest {
    padding-inline-start: 21px;
  }

  .cited {
    position: absolute;
    inset-block-start: 0;
    display: flex;
    flex-direction: column;
    inline-size: 280px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .cited button {
    display: flex;
    align-items: center;
    gap: 8px;
    inline-size: 100%;
    block-size: 28px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .cited button:hover {
    background: var(--color-control-hover);
  }

  .site {
    flex: none;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .title {
    overflow: hidden;
    font-size: var(--text-label);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cited .rest {
    padding: 4px 6px 0 28px;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .label-only .label {
    inline-size: 100%;
  }

  .label-only .summary {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  .stack .pages:hover .glass {
    box-shadow: var(--shadow-float);
  }

  .stack .page:hover .glass {
    translate: 0 -3px;
  }

  /* Surveyed from afar: the part's name set large, its pages as pictures, what search cited as sites. */
  .overview .summary,
  .tile .summary,
  .overview .caption,
  .tile .caption,
  .overview .title,
  .tile .cited,
  .tile strong,
  .overview .rest,
  .overview .count,
  .tile .count {
    display: none;
  }

  .overview .label,
  .tile .label {
    inline-size: 128px;
  }

  .overview .name {
    gap: 10px;
    padding: 0;
    translate: 0 -3px;
  }

  .overview strong {
    font-size: var(--survey);
    line-height: 30px;
    letter-spacing: -0.02em;
    overflow-wrap: normal;
    word-break: keep-all;
  }

  /* Surveyed, the stack beside a name shows the site; the name takes the column. */
  .overview .mark,
  .overview .glyph {
    display: none;
  }

  .overview .site {
    color: var(--color-text);
    font-size: 24px;
    font-weight: 500;
  }

  .overview .cited button {
    block-size: 36px;
    gap: 12px;
  }

  .tile .name {
    translate: 0 -12px;
  }

  .tile .mark,
  .tile .glyph {
    inline-size: 48px;
    block-size: 48px;
    scale: 3;
  }

  .overview .lines li {
    block-size: 36px;
    font-size: 22px;
  }

  .tile .lines {
    display: none;
  }

  .part.selected .label {
    box-shadow: 0 0 0 1.5px var(--color-ring);
  }

  .stack .pages:hover .count {
    opacity: 0;
  }
</style>

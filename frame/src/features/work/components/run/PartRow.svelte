<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import type { CanvasItem, PartPage } from "../../lib/canvas-model";
  import { PART, partLead, rowKey, stackSize } from "../../lib/run/part-size";
  import { reasonBadge } from "../../lib/work-human";
  import {
    CommandLineIcon,
    ComputerTerminal01Icon,
    FileEditIcon,
    File01Icon,
    Search01Icon,
  } from "../../lib/icons";
  import { askCard, partContent } from "./slots";
  import { serviceKey, serviceMark } from "$domain/connections";
  import { getContext, untrack } from "svelte";
  import { canvasBoard, type BoardActions } from "../../lib/canvas-context";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import PageFace from "./PageFace.svelte";
  import AgentOrb from "../cards/AgentOrb.svelte";
  import * as m from "$shared/i18n/messages";

  let {
    item,
    selected,
    onopen,
    onlist,
    onneed,
  }: {
    item: CanvasItem;
    selected: boolean;
    /** Opens one of the part's pages in the centre. */
    onopen: (page: string) => void;
    /** Opens everything the part read or cited. */
    onlist: () => void;
    /** Meets the part's need: `sign_in` opens the site, `again` runs the part again. */
    onneed?: (how: "meet" | "again") => void;
  } = $props();

  const part = $derived(item.part!);
  const shape = $derived(part.shape);
  /** The page it is on now leads, then the newest. */
  const ordered = $derived(
    [...part.pages].reverse().sort((a, b) => Number(b.live) - Number(a.live)),
  );
  const shown = $derived(ordered.slice(0, PART.shown));
  const stack = $derived(stackSize(part.pages.length));
  const working = $derived(part.state === "running" || part.state === "waiting");
  const cited = $derived(part.cited ?? []);
  const more = $derived(Math.max(0, (part.citedCount ?? 0) - cited.length));
  /** Folded, each window behind the front one peeks out right and below it. */
  const placeOf = (index: number) =>
    `translate(${index * PART.behindX}px, ${index * PART.behindY}px)`;
  /** Fanned out on hover, the windows stand side by side along the row. */
  const fanOf = (index: number) => `translate(${index * (PART.window + 16)}px, 0)`;
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
  const need = $derived(part.need);
  /** A sign-in opened: what is left is to run the part again. */
  let signing = $state(false);
  const needWords = $derived.by(() => {
    if (!need) return null;
    switch (need.kind) {
      case "sign_in":
        return signing
          ? { text: m.work_need_signed_in({ site: need.target }), action: m.work_need_again() }
          : { text: m.work_need_sign_in({ site: need.target }), action: m.work_ask_sign_in() };
      case "allow_site":
        return { text: m.work_need_allow_site({ site: need.target }), action: m.work_ask_allow() };
      case "allow_folder":
        return {
          text: m.work_need_allow_folder({ name: need.target }),
          action: m.work_ask_allow(),
        };
      case "use_connection":
        return {
          text: m.work_need_connection({ service: need.target }),
          action: m.work_need_use({ service: need.target }),
        };
      case "retry":
        return {
          text: need.target ? m.work_need_retry_site({ site: need.target }) : m.work_need_retry(),
          action: m.work_need_again(),
        };
    }
  });
  const board = getContext<BoardActions | undefined>(canvasBoard);
  // The row's own height: the taller of its name and its work, so rows never overlap.
  let labelBody = $state<HTMLElement>();
  let workBody = $state<HTMLElement>();
  $effect(() => {
    const label = labelBody;
    const work = workBody;
    if (!label) return;
    let reported = 0;
    const report = () => {
      const height = Math.ceil(Math.max(label.offsetHeight, work?.offsetHeight ?? 0));
      if (!height || height === reported) return;
      reported = height;
      untrack(() => board?.measure(rowKey(item.id), 0, false, height));
    };
    report();
    const observer = new ResizeObserver(() => requestAnimationFrame(report));
    observer.observe(label);
    if (work) observer.observe(work);
    return () => observer.disconnect();
  });
  /** A one-word name longer than its column is set smaller rather than cut. */
  const fitted = $derived.by(() => {
    const longest = Math.max(...part.title.split(/\s+/u).map((word) => word.length), 1);
    return Math.max(11, Math.min(13, Math.floor((PART.label - 23) / (longest * 0.56))));
  });
</script>

<section
  class="part work-drag-handle {shape}"
  class:label-only={shape === "label"}
  class:selected
  class:working
  aria-label={part.title}
  data-part={item.id}
>
  <div class="label" bind:this={labelBody}>
    <button
      type="button"
      class="name nodrag"
      style:--fitted="{fitted}px"
      onclick={(event) => {
        event.stopPropagation();
        onlist();
      }}
    >
      {#if part.helper === "research"}<span class="glyph"
          ><Icon icon={Search01Icon} size={14} /></span
        >{:else if part.helper === "computer"}<span class="glyph"
          ><Icon icon={ComputerTerminal01Icon} size={14} /></span
        >{:else if part.helper === "connection"}<span class="glyph"
          ><Icon icon={serviceMark(serviceKey(part.connection, part.title))} size={14} /></span
        >{:else}<span class="mark"
          ><HostGlyph host={part.host ?? ""} size={16} loading={working} initial={false} /></span
        >{/if}
      <strong>{part.title}</strong>
    </button>
    {#if part.presence !== undefined}<span class="presence" aria-hidden="true"
        ><AgentOrb seed={part.presence} size={14} ring /></span
      >{/if}
    {#if needWords}<p class="summary need-text">{needWords.text}</p>
      <button
        type="button"
        class="need nodrag nopan"
        onclick={(event) => {
          event.stopPropagation();
          if (need?.kind === "sign_in" && !signing) {
            signing = true;
            onneed?.("meet");
          } else onneed?.(need?.kind === "sign_in" || need?.kind === "retry" ? "again" : "meet");
        }}>{needWords.action}</button
      >
    {:else if part.summary}<p class="summary" class:turn={part.state === "waiting"}>
        {part.summary}
      </p>{/if}
  </div>

  {#if shape === "ask" && part.ask}
    <div class="slot" style:inset-inline-start="{partLead}px" data-part-ask={item.id}>
      <div bind:this={workBody}>
        {#if asking}{#await asking() then view}<view.default {...part.ask.props} />{/await}{/if}
      </div>
    </div>
  {:else if shape === "helper"}
    <div class="slot" style:inset-inline-start="{partLead}px">
      <div bind:this={workBody}>
        {#if content}{#await content() then view}<view.default
              id={item.id}
              {part}
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
    </div>
  {:else if shape === "pages"}
    <div
      class="stack"
      class:fans={shown.length > 1}
      style:inset-inline-start="{partLead}px"
      style:inline-size="{stack.width}px"
      style:block-size="{stack.height}px"
    >
      {#each shown as page, index (page.id)}
        {@render window(page, index)}
      {/each}
      {#if part.pages.length > 1}<span class="count" style:inset-inline-start="{stack.width}px"
          >{part.pages.length}</span
        >{/if}
    </div>
  {:else if shape === "sources"}
    <div class="slot" style:inset-inline-start="{partLead}px">
      <ul class="cited" bind:this={workBody}>
        {#each cited as row (row.key)}<li>
            <button
              type="button"
              class="nodrag"
              title={row.title}
              onclick={(event) => {
                event.stopPropagation();
                onlist();
              }}
            >
              <span class="site-mark"
                ><HostGlyph host={row.where} url={row.url} size={14} initial={false} /></span
              >
              <span class="site">{row.title}</span>
            </button>
          </li>{/each}
        {#if more}<li class="rest">{m.work_part_more_cited({ count: more })}</li>{/if}
      </ul>
    </div>
  {/if}
</section>

{#snippet window(page: PartPage, index: number)}
  <button
    type="button"
    class="page nodrag"
    class:behind={index > 0}
    style:--place={placeOf(index)}
    style:--fan={fanOf(index)}
    style:z-index={PART.shown - index}
    style:inline-size="{PART.window}px"
    style:block-size="{PART.bar + PART.frame}px"
    title={page.title || page.url}
    aria-label={page.title || page.url}
    onclick={(event) => {
      event.stopPropagation();
      onopen(page.id);
    }}
  >
    <PageFace
      url={page.url}
      title={page.title}
      frame={page.frame}
      host={part.host ?? ""}
      live={page.live}
    />
    {#if page.human?.phase === "waiting_for_human"}<span class="needs"
        ><span class="why">{reasonBadge(page.human.reason)}</span><span
          class="help"
          role="presentation">{m.work_human_help()}</span
        ></span
      >{/if}
  </button>
{/snippet}

<style>
  .part {
    position: relative;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    color: var(--color-text);
  }

  .label {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    box-sizing: border-box;
    inline-size: 144px;
  }

  .label-only .label {
    inline-size: 100%;
  }

  .name {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    min-inline-size: 0;
    padding: 3px 0;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .name:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
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
    font-size: var(--fitted, var(--text-body));
    font-weight: 600;
    line-height: 18px;
    letter-spacing: -0.005em;
    text-wrap: balance;
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
    margin: 0;
    padding-inline-start: 23px;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
    text-wrap: pretty;
  }

  .summary.turn {
    color: var(--color-text);
    font-weight: 600;
  }

  /* What the part needs, in a sentence, and the one control that meets it. */
  .need-text {
    color: var(--color-text);
  }

  .need {
    align-self: flex-start;
    margin: 6px 0 0 23px;
    padding: 4px 12px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 600;
    line-height: 16px;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .need:hover {
    background: var(--color-lit-hover);
  }

  .need:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .part.selected .name {
    box-shadow: 0 0 0 1.5px var(--color-ring);
  }

  .slot {
    position: absolute;
    inset-block-start: 0;
    inset-inline-end: 0;
  }

  /* The part's pages as windows: the front one whole, the ones behind it peeking out. */
  .stack {
    position: absolute;
    inset-block-start: 0;
  }

  .page {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    display: block;
    padding: 0;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
    transform: var(--place);
    transition: transform var(--motion-slow) var(--ease-emphasized);
  }

  .page:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  /* Hovering the stack fans its windows out along the row, over what follows. */
  .fans:hover .page {
    transform: var(--fan);
  }

  .fans .page:hover {
    translate: 0 -3px;
    transition:
      transform var(--motion-slow) var(--ease-emphasized),
      translate var(--motion-base) var(--ease-spring);
  }

  .count {
    position: absolute;
    inset-block-start: 0;
    z-index: 4;
    translate: -60% -40%;
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
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .fans:hover .count {
    opacity: 0;
  }

  .needs {
    position: absolute;
    inset: auto 8px 8px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 4px 4px 4px 10px;
    border-radius: var(--radius-control-compact);
    background: var(--color-float);
    box-shadow: var(--shadow-menu);
    color: var(--color-text);
    font-size: var(--text-label);
    font-weight: 600;
    line-height: 16px;
  }

  .help {
    padding: 3px 10px;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
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
    white-space: nowrap;
    mask-image: linear-gradient(to right, black calc(100% - 20px), transparent);
  }

  .lines .command span {
    font-family: var(--font-mono);
  }

  .lines .rest {
    padding-inline-start: 21px;
  }

  .cited {
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

  .site-mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 16px;
    block-size: 16px;
  }

  .site {
    color: var(--color-text);
    font-size: var(--text-body);
  }

  .cited .rest {
    padding: 4px 6px 0 28px;
    color: var(--color-muted);
    font-size: var(--text-label);
  }
</style>

<script lang="ts">
  import { getContext, onMount } from "svelte";
  import FavIcon from "$shared/ui/FavIcon";
  import Icon from "$shared/ui/Icon";
  import { siteMark } from "../cards/HostGlyph.svelte";
  import Popover from "./Popover.svelte";
  import {
    DIAGRAM,
    arrowHead,
    diagramKindLabel,
    diagramLayout,
    flowPath,
    partAlong,
    plateHeight,
    plateWidth,
  } from "../../lib/diagram";
  import { roleGlyph } from "../../lib/diagram-icons";
  import { PART_TEXT, lineCount } from "../../lib/diagram-text";
  import { vendorHost } from "../../lib/vendors";
  import { canvasProbe, type BoardActions } from "../../lib/canvas-context";
  import type { DiagramBlock } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let {
    block,
    diagram: given,
    width,
    actions,
    ask: asking,
  }: {
    block?: DiagramBlock;
    diagram?: DiagramBlock["diagram"];
    /** The room the picture has; a wider picture is drawn smaller to fit. */
    width: number;
    actions?: BoardActions;
    ask?: (subject: string) => void;
  } = $props();
  const probe = getContext<((origin: string) => void) | undefined>(canvasProbe);
  const diagram = $derived(given ?? block!.diagram);
  const ask = $derived(asking ?? actions?.ask);
  const layout = $derived(diagramLayout(diagram));
  const bounds = $derived(layout.bounds);
  const scale = $derived(Math.min(1, width / Math.max(1, bounds.width)));
  const { width: W, height: H } = DIAGRAM.node;
  const nodes = $derived(
    diagram.nodes.flatMap((node) => {
      const at = layout.at[node.id];
      if (!at) return [];
      const vendor = vendorHost(node.vendor ?? undefined, node.name, node.note ?? "") ?? "";
      return [{ ...node, x: at.x - bounds.x, y: at.y - bounds.y, vendor }];
    }),
  );
  const byId = $derived(new Map(nodes.map((node) => [node.id, node])));
  const flows = $derived(
    Object.entries(layout.flows).flatMap(([index, flow]) => {
      const edge = diagram.edges[Number(index)];
      return edge ? [{ index: Number(index), ...flow, label: edge.label?.trim() ?? "" }] : [];
    }),
  );
  /** The part looked at: under the pointer, holding the keyboard, or open. */
  let hovered = $state<string | null>(null);
  let opened = $state<string | null>(null);
  const focus = $derived(opened ?? hovered);
  const lit = (flow: { from: string; to: string }) =>
    !!focus && (flow.from === focus || flow.to === focus);
  const near = $derived(
    new Set(
      focus
        ? [focus, ...flows.filter(lit).flatMap((flow) => [flow.from, flow.to])]
        : nodes.map((node) => node.id),
    ),
  );
  /** Names on their lines: at rest where they found room, and those of a looked-at part's flows. */
  const plates = $derived.by(() => {
    const seen: string[] = [];
    return flows.flatMap((flow) => {
      if (!flow.label || !flow.plate) return [];
      if (focus ? !lit(flow) : !flow.resting) return [];
      const x = flow.plate.x - bounds.x;
      const y = flow.plate.y - bounds.y;
      // Flows that share a run and a name show it once.
      const key = `${Math.round(x)}:${Math.round(y)}:${flow.label}`;
      if (seen.includes(key)) return [];
      seen.push(key);
      return [{ index: flow.index, x, y, label: flow.label, quiet: flow.quiet }];
    });
  });
  const shift = (points: readonly { x: number; y: number }[]) =>
    points.map((point) => ({ x: point.x - bounds.x, y: point.y - bounds.y }));
  /** Room for a part's words beside its mark. */
  const ROOM = (W - 24 - 36) * 0.94;
  /** A part's note stands only in the lines its name leaves, and only whole; the popover has it all. */
  function note(node: { name: string; note?: string | null }): string {
    const text = node.note?.trim() ?? "";
    if (!text) return "";
    const named = lineCount(node.name, ROOM, PART_TEXT.name);
    const room = named <= 1 ? 2 : named === 2 ? 1 : 0;
    return lineCount(text, ROOM, PART_TEXT.note) <= room ? text : "";
  }
  const mark = (vendor: string) => (vendor ? siteMark(`https://${vendor}`) : null);
  onMount(() => {
    for (const node of nodes)
      if (node.vendor && !mark(node.vendor)) probe?.(`https://${node.vendor}`);
  });
  type Link = { way: "out" | "in"; other: string; label: string };
  const joined = (id: string): Link[] =>
    diagram.edges.flatMap((edge): Link[] =>
      edge.from === id && edge.to !== id
        ? [{ way: "out", other: edge.to, label: edge.label?.trim() ?? "" }]
        : edge.to === id && edge.from !== id
          ? [{ way: "in", other: edge.from, label: edge.label?.trim() ?? "" }]
          : [],
    );
  let host = $state<HTMLElement>();
  function keys(event: KeyboardEvent, id: string) {
    if (!event.key.startsWith("Arrow")) return;
    const from = byId.get(id);
    if (!from) return;
    const next = partAlong(
      { x: from.x + W / 2, y: from.y + H / 2 },
      joined(id).flatMap(({ other }) => {
        const node = byId.get(other);
        return node ? [{ id: other, at: { x: node.x + W / 2, y: node.y + H / 2 } }] : [];
      }),
      event.key,
    );
    if (!next) return;
    event.preventDefault();
    event.stopPropagation();
    host?.querySelector<HTMLElement>(`[data-part="${CSS.escape(next)}"]`)?.focus();
  }
</script>

<div
  class="diagram"
  style:inline-size={`${bounds.width * scale}px`}
  style:block-size={`${bounds.height * scale}px`}
>
  <div
    class="picture"
    bind:this={host}
    style:inline-size={`${bounds.width}px`}
    style:block-size={`${bounds.height}px`}
    style:transform={scale < 1 ? `scale(${scale})` : undefined}
  >
    {#each layout.tiers as tier (tier.name)}<div
        class="tier"
        style:inset-block-start={`${tier.y - bounds.y}px`}
        style:block-size={`${tier.height}px`}
        style:inline-size={`${layout.gutter - 20}px`}
      >
        <span>{tier.name}</span>
      </div>{/each}
    <svg class="flows" width={bounds.width} height={bounds.height} aria-hidden="true">
      {#each flows as flow (flow.index)}{@const points = shift(flow.points)}<g
          class="flow"
          class:quiet={flow.quiet}
          class:back={flow.back}
          class:lit={lit(flow)}
          class:away={!!focus && !lit(flow)}
          >{#if !flow.twin}<path d={flowPath(points, 5)} />{/if}<path
            class="head"
            d={arrowHead(points)}
          /></g
        >{/each}
    </svg>
    {#each plates as plate (plate.index)}<span
        class="plate"
        class:quiet={plate.quiet && !focus}
        style:inset-inline-start={`${plate.x}px`}
        style:inset-block-start={`${plate.y}px`}
        style:inline-size={`${plateWidth(plate.label)}px`}
        style:block-size={`${plateHeight(plate.label)}px`}>{plate.label}</span
      >{/each}
    {#each nodes as node (node.id)}{@const logo = mark(node.vendor)}{@const said = note(node)}
      <div
        class="part"
        class:dim={!near.has(node.id)}
        style:inset-inline-start={`${node.x}px`}
        style:inset-block-start={`${node.y}px`}
        style:inline-size={`${W}px`}
        style:block-size={`${H}px`}
      >
        <button
          type="button"
          class="face nodrag nopan"
          data-part={node.id}
          aria-expanded={opened === node.id}
          onpointerenter={() => (hovered = node.id)}
          onpointerleave={() => hovered === node.id && (hovered = null)}
          onfocus={() => (hovered = node.id)}
          onblur={() => hovered === node.id && (hovered = null)}
          onkeydown={(event) => keys(event, node.id)}
          onclick={() => (opened = opened === node.id ? null : node.id)}
        >
          <span class="mark" class:logo={!!logo}
            >{#if logo}<FavIcon image={logo.image} tone={logo.tone} size={18} />{:else}<Icon
                icon={roleGlyph(node)}
                size={16}
              />{/if}</span
          >
          <span class="words">
            <strong class="name">{node.name}</strong>
            {#if said}<span class="line">{said}</span>{/if}
          </span>
        </button>
        {#if opened === node.id}
          <Popover label={node.name} onclose={() => (opened = null)}>
            <p class="pop-kind">
              {diagramKindLabel(node.kind)}{#if node.vendor}<span> · {node.vendor}</span>{/if}
            </p>
            <p class="pop-name">{node.name}</p>
            {#if node.note}<p class="pop-note">{node.note}</p>{/if}
            {#if joined(node.id).length}<ul class="pop-links" aria-label={m.work_board_connects()}>
                {#each joined(node.id) as link, index (index)}<li>
                    <span class="way">{link.way === "out" ? "→" : "←"}</span>
                    <span class="other">{byId.get(link.other)?.name ?? link.other}</span>
                    {#if link.label}<span class="via">{link.label}</span>{/if}
                  </li>{/each}
              </ul>{/if}
            {#if ask}<button
                type="button"
                class="ask"
                onclick={() => {
                  opened = null;
                  ask(node.name);
                }}>{m.work_board_ask()}</button
              >{/if}
          </Popover>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .diagram {
    position: relative;
    margin-inline: auto;
  }

  .picture {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    transform-origin: 0 0;
  }

  /* A tier is alignment: its name beside its first row, a hairline down the rows it holds. */
  .tier {
    position: absolute;
    inset-inline-start: 0;
    box-sizing: border-box;
    padding-block-start: 7px;
    border-inline-end: 1px solid var(--color-border);
  }

  .tier span {
    display: block;
    padding-inline-end: 12px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.07em;
    line-height: 15px;
    text-transform: uppercase;
    text-wrap: balance;
  }

  .flows {
    position: absolute;
    inset: 0;
    overflow: visible;
    pointer-events: none;
  }

  .flow path {
    fill: none;
    stroke: var(--color-border-strong);
    stroke-width: 1.25;
    transition:
      stroke var(--motion-fast) var(--ease-out),
      opacity var(--motion-fast) var(--ease-out);
  }

  .flow path.head {
    fill: var(--color-border-strong);
    stroke: none;
  }

  /* A side channel steps back; a reply runs dashed against the work's own direction. */
  .flow.quiet {
    opacity: 0.55;
  }

  .flow.lit {
    opacity: 1;
  }

  .flow.lit path {
    stroke: var(--color-muted);
    stroke-width: 1.5;
  }

  .flow.lit path.head {
    fill: var(--color-muted);
    stroke: none;
  }

  .flow.back path:not(.head) {
    stroke-dasharray: 3 3;
  }

  .flow.away {
    opacity: 0.18;
  }

  .plate {
    position: absolute;
    display: grid;
    box-sizing: border-box;
    place-items: center;
    padding: 0 6px;
    translate: -50% -50%;
    border-radius: var(--radius-inset);
    background: var(--color-surface);
    color: var(--color-label-secondary);
    font-size: var(--text-caption);
    font-weight: 500;
    line-height: 15px;
    text-align: center;
    text-wrap: balance;
    pointer-events: none;
  }

  .plate.quiet {
    color: var(--color-muted);
  }

  .part {
    position: absolute;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .part.dim {
    opacity: 0.4;
  }

  .part:has(:global([role="dialog"])) {
    z-index: 3;
  }

  .face {
    display: flex;
    align-items: center;
    gap: 8px;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-row);
    background: var(--color-raised);
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: default;
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .face:hover,
  .face[aria-expanded="true"] {
    box-shadow:
      inset 0 0 0 1px var(--color-muted),
      var(--shadow-raised);
  }

  .face:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-label-secondary);
  }

  .words {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .name {
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
    overflow-wrap: anywhere;
    text-wrap: balance;
  }

  /* Wrapped greedily, as its lines were counted. */
  .line {
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 15px;
  }

  .pop-kind {
    margin: 0;
    color: var(--color-faint);
    font-size: var(--text-label);
  }

  .pop-name {
    margin: 2px 0 0;
    font-weight: 600;
  }

  .pop-note {
    margin: 6px 0 0;
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    line-height: 17px;
  }

  .pop-links {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 12px 0 0;
    padding: 10px 0 0;
    border-block-start: 1px solid var(--color-border);
    list-style: none;
    font-size: var(--text-label);
  }

  .pop-links li {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-inline-size: 0;
  }

  .way {
    flex: none;
    color: var(--color-faint);
  }

  .other {
    font-weight: 500;
  }

  .via {
    color: var(--color-faint);
  }

  .ask {
    block-size: 26px;
    margin-block-start: 14px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-control);
    color: var(--color-on-control);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .ask:hover {
    background: var(--color-control-hover);
  }
</style>

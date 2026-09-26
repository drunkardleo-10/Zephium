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
  import { diagramGlyph } from "../../lib/diagram-glyphs";
  import { vendorHost } from "../../lib/vendors";
  import { canvasProbe, type BoardActions } from "../../lib/canvas-context";
  import type { DiagramBlock } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let {
    block,
    width,
    actions,
  }: {
    block: DiagramBlock;
    /** The room the picture has; a wider picture is drawn smaller to fit. */
    width: number;
    actions?: BoardActions;
  } = $props();
  const probe = getContext<((origin: string) => void) | undefined>(canvasProbe);
  const diagram = $derived(block.diagram);
  const layout = $derived(diagramLayout(diagram));
  const bounds = $derived(layout.bounds);
  const scale = $derived(Math.min(1, width / Math.max(1, bounds.width)));
  const { width: W, height: H } = DIAGRAM.node;
  const nodes = $derived(
    diagram.nodes.flatMap((node) => {
      const at = layout.at[node.id];
      if (!at) return [];
      const vendor = vendorHost(node.vendor, node.name, node.note) ?? "";
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
  type Plate = { index: number; x: number; y: number; w: number; h: number; label: string };
  const plates = $derived(
    flows.flatMap((flow): Plate[] => {
      if (!flow.label || !flow.plate) return [];
      const w = plateWidth(flow.label);
      const h = plateHeight(flow.label);
      return [
        {
          index: flow.index,
          x: flow.plate.x - bounds.x - w / 2,
          y: flow.plate.y - bounds.y - h / 2,
          w,
          h,
          label: flow.label,
        },
      ];
    }),
  );
  /** Names at rest: a primary flow's, where it covers no part and no other name. */
  const resting = $derived.by(() => {
    const kept: Plate[] = [];
    const clear = (a: Plate, b: { x: number; y: number; w: number; h: number }) =>
      a.x + a.w + 2 <= b.x || b.x + b.w + 2 <= a.x || a.y + a.h + 2 <= b.y || b.y + b.h + 2 <= a.y;
    for (const plate of plates) {
      if (!layout.flows[plate.index]?.primary) continue;
      if (!nodes.every((node) => clear(plate, { x: node.x, y: node.y, w: W, h: H }))) continue;
      if (!kept.every((other) => clear(plate, other))) continue;
      kept.push(plate);
    }
    return new Set(kept.map((plate) => plate.index));
  });
  const shown = (plate: Plate) => {
    const flow = flows.find((entry) => entry.index === plate.index);
    return focus ? !!flow && lit(flow) : resting.has(plate.index);
  };
  /** A tier is alignment and a quiet name above its column, never a box. */
  const tiers = $derived.by(() => {
    if (!nodes.length) return [];
    const top = Math.min(...nodes.map((node) => node.y));
    return layout.layers.flatMap((layer) => {
      const xs = layer.nodes.flatMap((id) => {
        const node = byId.get(id);
        return node ? [node.x] : [];
      });
      return xs.length ? [{ name: layer.name, x: Math.min(...xs), y: Math.max(0, top - 24) }] : [];
    });
  });
  const shift = (points: readonly { x: number; y: number }[]) =>
    points.map((point) => ({ x: point.x - bounds.x, y: point.y - bounds.y }));
  const mark = (vendor: string) => (vendor ? siteMark(`https://${vendor}`) : null);
  onMount(() => {
    for (const node of nodes)
      if (node.vendor && !mark(node.vendor)) probe?.(`https://${node.vendor}`);
  });
  type Link = { way: "out" | "in"; other: string; label: string };
  const joined = (id: string): Link[] =>
    flows.flatMap((flow): Link[] =>
      flow.from === id
        ? [{ way: "out", other: flow.to, label: flow.label }]
        : flow.to === id
          ? [{ way: "in", other: flow.from, label: flow.label }]
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
    {#each tiers as tier (tier.name)}<span
        class="tier"
        style:inset-inline-start={`${tier.x}px`}
        style:inset-block-start={`${tier.y}px`}>{tier.name}</span
      >{/each}
    <svg class="flows" width={bounds.width} height={bounds.height} aria-hidden="true">
      {#each flows as flow (flow.index)}{@const points = shift(flow.points)}<g
          class="flow"
          class:primary={flow.primary}
          class:lit={lit(flow)}
          class:quiet={!!focus && !lit(flow)}
          ><path d={flowPath(points, 5)} /><path class="head" d={arrowHead(points)} /></g
        >{/each}
    </svg>
    {#each plates as plate (plate.index)}{#if shown(plate)}<span
          class="plate"
          style:inset-inline-start={`${plate.x + plate.w / 2}px`}
          style:inset-block-start={`${plate.y + plate.h / 2}px`}
          title={plate.label}>{plate.label}</span
        >{/if}{/each}
    {#each nodes as node (node.id)}{@const logo = mark(node.vendor)}
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
            >{#if logo}<FavIcon image={logo.image} tone={logo.tone} size={20} />{:else}<Icon
                icon={diagramGlyph(node.kind)}
                size={14}
              />{/if}</span
          >
          <span class="words">
            <strong class="name">{node.name}</strong>
            <span class="line">{node.note || diagramKindLabel(node.kind)}</span>
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
            {#if actions}<button
                type="button"
                class="ask"
                onclick={() => {
                  opened = null;
                  actions.ask(node.name);
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

  .tier {
    position: absolute;
    color: var(--color-muted);
    font-size: var(--text-label);
    font-weight: 500;
    letter-spacing: 0.04em;
    line-height: 16px;
    text-transform: uppercase;
    white-space: nowrap;
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

  .flow:not(.primary) {
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

  .flow.quiet {
    opacity: 0.2;
  }

  .plate {
    position: absolute;
    box-sizing: border-box;
    max-inline-size: 220px;
    padding: 1px 6px;
    translate: -50% -50%;
    inline-size: max-content;
    border-radius: var(--radius-inset);
    background: var(--color-surface);
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    line-height: 16px;
    text-align: center;
    overflow-wrap: anywhere;
    pointer-events: none;
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
    gap: 10px;
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

  .mark.logo {
    background: transparent;
  }

  .words {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .name {
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .line {
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 15px;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    overflow: hidden;
    color: var(--color-faint);
    text-overflow: ellipsis;
    white-space: nowrap;
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

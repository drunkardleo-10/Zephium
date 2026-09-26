<script lang="ts">
  import type { ArtifactContent } from "$shared/ui/data/Artifact/artifact";
  import FavIcon from "$shared/ui/FavIcon";
  import Icon from "$shared/ui/Icon";
  import { siteMark } from "./cards/HostGlyph.svelte";
  import { diagramGlyph } from "../lib/diagram-glyphs";
  import {
    DIAGRAM,
    PLATE,
    arrowHead,
    diagramKindLabel,
    diagramLayout,
    flowPath,
    plateHeight,
  } from "../lib/diagram";
  import { vendorHost } from "../lib/vendors";
  import * as m from "$shared/i18n/messages";
  type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;
  let { content }: { content: Diagram } = $props();
  /** The picture reads larger in the lift than on the canvas. */
  const SCALE = 1.25;
  const layout = $derived(diagramLayout(content));
  const box = $derived(layout.bounds);
  const names = $derived(new Map(content.nodes.map((node) => [node.id, node.name])));
  const vendors = $derived(
    new Map(
      content.nodes.map((node) => {
        const host = vendorHost(node.vendor, node.name, node.note);
        return [node.id, host ? { host, mark: siteMark(`https://${host}`) } : null] as const;
      }),
    ),
  );
  /** A part or a flow picked in the lists, or a part under the pointer in the picture. */
  let picked = $state<{ part: string } | { flow: number } | null>(null);
  let pointed = $state<string | null>(null);
  const focus = $derived(pointed ? { part: pointed } : picked);
  const flowLit = (index: number) => {
    if (!focus) return false;
    if ("flow" in focus) return focus.flow === index;
    const flow = layout.flows[index];
    return !!flow && (flow.from === focus.part || flow.to === focus.part);
  };
  const partLit = (id: string) => {
    if (!focus) return true;
    if ("part" in focus)
      return (
        focus.part === id ||
        Object.values(layout.flows).some(
          (flow) =>
            (flow.from === focus.part && flow.to === id) ||
            (flow.to === focus.part && flow.from === id),
        )
      );
    const flow = layout.flows[focus.flow];
    return !!flow && (flow.from === id || flow.to === id);
  };
  const flowState = (index: number, primary: boolean) =>
    focus ? (flowLit(index) ? "lit" : "quiet") : primary ? "rest" : "quiet";
  const toggle = (next: { part: string } | { flow: number }) => {
    const same =
      picked &&
      (("part" in next && "part" in picked && picked.part === next.part) ||
        ("flow" in next && "flow" in picked && picked.flow === next.flow));
    picked = same ? null : next;
  };
</script>

<div class="diagram-lift">
  <div class="well" role="img" aria-label={m.work_diagram_picture()}>
    <div
      class="stage"
      style:inline-size="{box.width * SCALE}px"
      style:block-size="{box.height * SCALE}px"
    >
      <div
        class="scaled"
        style:inline-size="{box.width}px"
        style:block-size="{box.height}px"
        style:transform="scale({SCALE})"
      >
        {#each layout.bands as band (band.name)}<div
            class="band"
            style:inset-inline-start="{band.x - box.x}px"
            style:inset-block-start="{band.y - box.y}px"
            style:inline-size="{band.width}px"
            style:block-size="{band.height}px"
          >
            <span>{band.name}</span>
          </div>{/each}
        <svg
          class="flows"
          width={box.width}
          height={box.height}
          viewBox="{box.x} {box.y} {box.width} {box.height}"
          aria-hidden="true"
        >
          {#each Object.entries(layout.flows) as [index, flow] (index)}<g
              class="flow {flowState(Number(index), flow.primary)}"
              ><path d={flowPath(flow.points, 5)} fill="none" /><path
                class="head"
                d={arrowHead(flow.points, 5)}
              /></g
            >{/each}
        </svg>
        {#each content.nodes as node (node.id)}{@const at = layout.at[node.id]}{@const vendor =
            vendors.get(node.id)}{#if at}<div
              class="part"
              class:dimmed={!partLit(node.id)}
              role="presentation"
              style:inset-inline-start="{at.x - box.x}px"
              style:inset-block-start="{at.y - box.y}px"
              style:inline-size="{DIAGRAM.node.width}px"
              style:block-size="{DIAGRAM.node.height}px"
              onpointerenter={() => (pointed = node.id)}
              onpointerleave={() => (pointed = null)}
            >
              {#if vendor?.mark}<FavIcon
                  image={vendor.mark.image}
                  tone={vendor.mark.tone}
                  size={24}
                />{/if}
              <span class="words">
                <strong class="name" title={node.name}
                  >{#if !vendor?.mark}<span class="glyph"
                      ><Icon icon={diagramGlyph(node.kind)} size={12} /></span
                    >{/if}{node.name}</strong
                >
                <span class="note">{node.note || diagramKindLabel(node.kind)}</span>
              </span>
            </div>{/if}{/each}
        {#each content.edges as edge, index (index)}{@const flow =
            layout.flows[
              index
            ]}{#if flow?.plate && edge.label?.trim() && flowState(index, flow.primary) !== "quiet"}<span
              class="plate"
              class:tall={plateHeight(edge.label) > PLATE.height}
              class:lit={flowLit(index)}
              title={edge.label}
              style:inset-inline-start="{flow.plate.x - box.x}px"
              style:inset-block-start="{flow.plate.y - box.y}px">{edge.label}</span
            >{/if}{/each}
      </div>
    </div>
  </div>
  <aside class="lists">
    <h3>{m.work_diagram_parts_heading()}</h3>
    <ul>
      {#each content.nodes as node (node.id)}{@const vendor = vendors.get(node.id)}
        <li>
          <button
            type="button"
            class="row"
            aria-pressed={!!picked && "part" in picked && picked.part === node.id}
            onclick={() => toggle({ part: node.id })}
          >
            <span class="row-mark" aria-hidden="true"
              >{#if vendor?.mark}<FavIcon
                  image={vendor.mark.image}
                  tone={vendor.mark.tone}
                  size={16}
                />{:else}<Icon icon={diagramGlyph(node.kind)} size={14} />{/if}</span
            >
            <span class="row-words">
              <span class="row-title">{node.name}</span>
              <span class="row-meta"
                >{diagramKindLabel(node.kind)}{#if vendor}{` · ${vendor.host}`}{/if}</span
              >
              {#if node.note}<span class="row-note">{node.note}</span>{/if}
            </span>
          </button>
        </li>{/each}
    </ul>
    <h3>{m.work_diagram_flows_heading()}</h3>
    <ul>
      {#each content.edges as edge, index (index)}{#if layout.flows[index]}<li>
            <button
              type="button"
              class="row"
              aria-pressed={!!picked && "flow" in picked && picked.flow === index}
              onclick={() => toggle({ flow: index })}
            >
              <span class="row-words">
                <span class="row-title"
                  >{names.get(edge.from)} → {names.get(edge.to)}{#if edge.label?.trim()}<span
                      class="row-meta">: {edge.label}</span
                    >{/if}</span
                >
              </span>
            </button>
          </li>{/if}{/each}
    </ul>
  </aside>
</div>

<style>
  /* The picture beside its parts and flows; one column once the lift is narrow. */
  .diagram-lift {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 280px;
    gap: 16px;
    align-items: start;
    container-type: inline-size;
  }

  @container (width < 760px) {
    .lists {
      grid-column: 1 / -1;
    }

    .well {
      grid-column: 1 / -1;
    }
  }

  .well {
    max-block-size: 70vh;
    overflow: auto;
    border-radius: var(--radius-card);
    background: color-mix(in srgb, var(--color-fill) 60%, transparent);
    box-shadow: inset 0 0 0 1px var(--color-border);
  }

  .stage {
    position: relative;
    margin: 16px;
  }

  .scaled {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    transform-origin: 0 0;
  }

  .band {
    position: absolute;
    box-sizing: border-box;
    padding: 8px 12px;
    border-radius: var(--radius-panel);
    background: color-mix(in srgb, var(--color-surface) 40%, transparent);
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
    line-height: 13px;
  }

  .flows {
    position: absolute;
    inset: 0;
    overflow: visible;
  }

  .flow {
    stroke: var(--color-border-strong);
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .flow .head {
    stroke: none;
    fill: var(--color-border-strong);
  }

  .flow.quiet {
    opacity: 0.35;
  }

  .flow.lit {
    stroke: var(--color-label-secondary);
  }

  .flow.lit .head {
    fill: var(--color-label-secondary);
  }

  .part {
    position: absolute;
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    padding: 0 12px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .part.dimmed {
    opacity: 0.6;
  }

  .words {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-inline-size: 0;
  }

  .name,
  .note {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    overflow-wrap: anywhere;
  }

  .name {
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
  }

  .note {
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 13px;
  }

  .glyph {
    display: inline-flex;
    margin-inline-end: 5px;
    color: var(--color-label-secondary);
    vertical-align: -1px;
  }

  /* Centred on its point of the line, as the canvas's plates are. */
  .plate {
    position: absolute;
    max-inline-size: 220px;
    translate: -50% -50%;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    box-sizing: border-box;
    padding: 1px 6px;
    overflow: hidden;
    overflow-wrap: anywhere;
    border-radius: var(--radius-capsule);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 15px;
    text-align: center;
  }

  .plate.tall {
    border-radius: var(--radius-inset);
  }

  .plate.lit {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
    color: var(--color-text);
  }

  .lists {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-inline-size: 0;
  }

  h3 {
    margin: 8px 0 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    inline-size: 100%;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .row:hover {
    background: var(--row-hover);
  }

  .row[aria-pressed="true"] {
    background: var(--row-active);
  }

  .row:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .row-mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 16px;
    block-size: 17px;
    color: var(--color-label-secondary);
  }

  .row-words {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-inline-size: 0;
  }

  .row-title {
    font-size: var(--text-label);
    font-weight: 500;
    line-height: 17px;
    overflow-wrap: anywhere;
  }

  .row-meta,
  .row-note {
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 400;
    line-height: 14px;
  }
</style>

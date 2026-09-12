<script lang="ts">
  import "@xyflow/svelte/dist/base.css";
  import { SvelteFlow, Background, type Edge } from "@xyflow/svelte";
  import { untrack, setContext } from "svelte";
  import { canvasInspection } from "../lib/canvas-context";
  import CanvasNode from "./CanvasNode.svelte";
  import CanvasControls from "./CanvasControls.svelte";
  import {
    reconcileNodes,
    validScene,
    validViewport,
    type CanvasItem,
    type CanvasLink,
    type CanvasView,
    type WorkNode,
  } from "../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let {
    items,
    links,
    initialView,
    oninspect,
    onviewchange,
  }: {
    items: readonly CanvasItem[];
    links: readonly CanvasLink[];
    initialView?: CanvasView;
    oninspect: (id: string) => void;
    onviewchange?: (view: CanvasView) => void;
  } = $props();
  setContext(canvasInspection, (id: string) => oninspect(id));
  let nodes = $state.raw<WorkNode[]>([]);
  const restoredViewport = untrack(() => validViewport(initialView?.viewport));
  let viewport = $state(restoredViewport ?? { x: 0, y: 0, zoom: 1 });
  let valid = $derived(validScene(items, links));
  const nodeTypes = { work: CanvasNode };
  let edges = $derived<Edge[]>(
    valid
      ? links.map((link) => ({
          id: link.id,
          source: link.source,
          target: link.target,
          type: "smoothstep",
          animated: false,
          deletable: false,
          selectable: false,
          style: link.kind === "reference" ? "stroke-dasharray: 5 5" : undefined,
        }))
      : [],
  );
  $effect(() => {
    const next = items;
    const ready = valid;
    nodes = untrack(() => (ready ? reconcileNodes(nodes, next, initialView?.positions) : []));
  });
  let publishedPositions = "";
  $effect(() => {
    if (!nodes.length || nodes.some((node) => node.dragging)) return;
    const key = JSON.stringify(nodes.map((node) => [node.id, node.position.x, node.position.y]));
    if (key !== publishedPositions) {
      publishedPositions = key;
      untrack(publishView);
    }
  });
  function publishView() {
    onviewchange?.({
      positions: Object.fromEntries(nodes.map((node) => [node.id, { ...node.position }])),
      viewport: { ...viewport },
    });
  }
</script>

<div class="work-canvas" aria-label={m.work_canvas_label()}>
  {#if valid && items.length}
    <SvelteFlow
      bind:nodes
      {edges}
      {nodeTypes}
      bind:viewport
      fitView={!restoredViewport}
      minZoom={0.2}
      maxZoom={2}
      nodeExtent={[
        [-1_000_000, -1_000_000],
        [1_000_000, 1_000_000],
      ]}
      nodesConnectable={false}
      onlyRenderVisibleElements={items.length >= 100}
      elementsSelectable
      deleteKey={[]}
      onnodeclick={({ node }) => oninspect(node.id)}
      onmoveend={publishView}
    >
      <Background patternColor="var(--color-border)" gap={24} />
      <CanvasControls />
    </SvelteFlow>
  {:else}<div class="canvas-empty" role="status">
      {valid ? m.work_canvas_empty() : m.work_canvas_unavailable()}
    </div>{/if}
</div>

<style>
  .work-canvas {
    width: 100%;
    height: 100%;
    min-height: 360px;
    background: var(--color-canvas);

    --xy-edge-stroke-default: var(--color-border-strong);
    --xy-edge-stroke-width-default: 1.5;
    --xy-handle-background-color-default: var(--color-border-strong);
    --xy-handle-border-color-default: var(--color-surface);
    --xy-selection-background-color-default: var(--color-accent-soft);
    --xy-selection-border-default: 1px solid var(--color-accent);
  }

  .work-canvas :global(.svelte-flow) {
    min-height: 360px;
  }

  .canvas-empty {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 24px;
    color: var(--color-muted);
  }
</style>

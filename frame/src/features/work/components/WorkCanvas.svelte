<script lang="ts">
  import "@xyflow/svelte/dist/base.css";
  import { SvelteFlow, Background, type Edge } from "@xyflow/svelte";
  import { untrack, setContext } from "svelte";
  import {
    canvasInspection,
    canvasResize,
    canvasAction,
    canvasEvidence,
    canvasFocusResult,
  } from "../lib/canvas-context";
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
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  let {
    items,
    links,
    initialView,
    fitBottomInset = 0,
    oninspect,
    onaction,
    onevidence,
    onviewchange,
  }: {
    items: readonly CanvasItem[];
    links: readonly CanvasLink[];
    initialView?: CanvasView;
    fitBottomInset?: number;
    oninspect: (id: string) => void;
    onaction?: (id: string) => void;
    onevidence?: (id: string, reference: EvidenceReference) => void;
    onviewchange?: (view: CanvasView) => void;
  } = $props();
  setContext(canvasEvidence, {
    get open() {
      return onevidence;
    },
  });
  setContext(canvasAction, (id: string) => onaction?.(id));
  setContext(canvasInspection, (id: string) => oninspect(id));
  let resizing = $state(false);
  setContext(canvasResize, (active: boolean) => (resizing = active));
  let nodes = $state.raw<WorkNode[]>([]);
  let canvasWidth = $state(0);
  let canvasHeight = $state(0);
  setContext(canvasFocusResult, (id: string) => {
    const node = nodes.find((node) => node.id === id && node.data.artifact);
    if (!node) return;
    viewport = {
      x: Math.max(24, (canvasWidth - (node.width ?? 480)) / 2) - node.position.x,
      y:
        110 +
        Math.max(0, (canvasHeight - fitBottomInset - 142 - (node.height ?? 360)) / 2) -
        node.position.y,
      zoom: 1,
    };
    publishView();
  });
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
          focusable: false,
          ariaLabel:
            link.kind === "dependency"
              ? m.work_env_dependency_label({
                  source: items.find((item) => item.id === link.source)?.title ?? "",
                  target: items.find((item) => item.id === link.target)?.title ?? "",
                })
              : m.work_env_reference_label({
                  source: items.find((item) => item.id === link.source)?.title ?? "",
                  target: items.find((item) => item.id === link.target)?.title ?? "",
                }),
          style: link.kind === "reference" ? "stroke-dasharray: 5 5" : undefined,
        }))
      : [],
  );
  $effect(() => {
    const next = items;
    const ready = valid;
    nodes = untrack(() =>
      ready ? reconcileNodes(nodes, next, initialView?.positions, initialView?.sizes) : [],
    );
  });
  let publishedPositions = "";
  $effect(() => {
    if (!nodes.length || resizing || nodes.some((node) => node.dragging)) return;
    const key = JSON.stringify(
      nodes.map((node) => [node.id, node.position.x, node.position.y, node.width, node.height]),
    );
    if (key !== publishedPositions) {
      publishedPositions = key;
      untrack(publishView);
    }
  });
  function publishView() {
    if (resizing) return;
    onviewchange?.({
      positions: Object.fromEntries(nodes.map((node) => [node.id, { ...node.position }])),
      viewport: { ...viewport },
      sizes: Object.fromEntries(
        nodes.map((node) => [
          node.id,
          { width: Math.round(node.width ?? 280), height: Math.round(node.height ?? 160) },
        ]),
      ),
    });
  }
</script>

<div
  class="work-canvas"
  bind:clientWidth={canvasWidth}
  bind:clientHeight={canvasHeight}
  aria-label={m.work_canvas_label()}
>
  {#if valid}
    <SvelteFlow
      bind:nodes
      {edges}
      {nodeTypes}
      bind:viewport
      fitView={items.length > 0 && !restoredViewport}
      minZoom={0.2}
      maxZoom={2}
      nodeExtent={[
        [-1_000_000, -1_000_000],
        [1_000_000, 1_000_000],
      ]}
      proOptions={{ hideAttribution: true }}
      nodesConnectable={false}
      edgesFocusable={false}
      ariaLabelConfig={{ "edge.a11yDescription.default": m.work_env_edge_readonly() }}
      onlyRenderVisibleElements={items.length >= 100}
      elementsSelectable
      deleteKey={[]}
      onnodeclick={({ node, event }) => {
        if (
          event.target instanceof Element &&
          event.target.closest(".artifact-body, button, a, input, textarea, select, summary")
        )
          return;
        oninspect(node.id);
      }}
      onmoveend={publishView}
    >
      <Background patternColor="var(--color-border)" gap={24} />
      <CanvasControls bottomInset={fitBottomInset} />
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

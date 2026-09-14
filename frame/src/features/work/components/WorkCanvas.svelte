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
    canvasOpen,
  } from "../lib/canvas-context";
  import CanvasNode from "./CanvasNode.svelte";
  import CanvasControls from "./CanvasControls.svelte";
  import {
    reconcileNodes,
    applyRemoteView,
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
    remoteView,
    authoritative,
    fitBottomInset = 0,
    fitTopInset = 0,
    virtualizeFrom = 24,
    oninspect,
    onopen,
    onaction,
    onevidence,
    onviewchange,
    onselectionchange,
    expose,
  }: {
    items: readonly CanvasItem[];
    links: readonly CanvasLink[];
    initialView?: CanvasView;
    remoteView?: { sequence: number; view: CanvasView };
    authoritative: ReadonlySet<string>;
    fitBottomInset?: number;
    fitTopInset?: number;
    virtualizeFrom?: number;
    oninspect: (id: string) => void;
    onopen?: (id: string) => void;
    onaction?: (id: string, action?: string) => void;
    onevidence?: (id: string, reference: EvidenceReference) => void;
    onviewchange?: (view: CanvasView) => void;
    onselectionchange?: (ids: string[]) => void;
    expose?: (api: { screenRect: (id: string) => DOMRect | null }) => void;
  } = $props();
  setContext(canvasEvidence, {
    get open() {
      return onevidence;
    },
  });
  setContext(canvasAction, (id: string, action?: string) => onaction?.(id, action));
  setContext(canvasInspection, (id: string) => oninspect(id));
  setContext(canvasOpen, (id: string) => onopen?.(id));
  let resizing = $state(false);
  setContext(canvasResize, (active: boolean) => (resizing = active));
  let nodes = $state.raw<WorkNode[]>([]);
  let canvasWidth = $state(0);
  let canvasHeight = $state(0);
  let host = $state<HTMLDivElement>();
  setContext(canvasFocusResult, (id: string) => {
    const node = nodes.find((node) => node.id === id);
    if (!node) return;
    viewport = {
      x: Math.max(24, (canvasWidth - (node.width ?? 480)) / 2) - node.position.x,
      y:
        fitTopInset +
        Math.max(0, (canvasHeight - fitBottomInset - fitTopInset - (node.height ?? 360)) / 2) -
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
  const positionKey = (list: WorkNode[]) =>
    JSON.stringify(
      list.map((node) => [node.id, node.position.x, node.position.y, node.width, node.height]),
    );
  let appliedRemote = 0;
  let deferredRemote: CanvasView | null = null;
  function applyRemote(view: CanvasView) {
    const next = applyRemoteView(nodes, view, authoritative);
    const restored = validViewport(view.viewport);
    if (next !== nodes) {
      publishedPositions = positionKey(next);
      nodes = next;
    }
    if (restored) viewport = restored;
  }
  $effect(() => {
    const remote = remoteView;
    if (!remote || remote.sequence === appliedRemote) return;
    appliedRemote = remote.sequence;
    untrack(() => {
      if (resizing || nodes.some((node) => node.dragging)) deferredRemote = remote.view;
      else applyRemote(remote.view);
    });
  });
  $effect(() => {
    if (!nodes.length || resizing || nodes.some((node) => node.dragging)) return;
    if (deferredRemote) {
      const view = deferredRemote;
      deferredRemote = null;
      untrack(() => applyRemote(view));
      return;
    }
    const key = positionKey(nodes);
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
  function screenRect(id: string): DOMRect | null {
    const node = nodes.find((node) => node.id === id);
    const origin = host?.getBoundingClientRect();
    if (!node || !origin) return null;
    const z = viewport.zoom;
    return new DOMRect(
      origin.left + node.position.x * z + viewport.x,
      origin.top + node.position.y * z + viewport.y,
      (node.width ?? 280) * z,
      (node.height ?? 160) * z,
    );
  }
  $effect(() => {
    expose?.({ screenRect });
  });
  let lastClick = { id: "", at: 0 };
</script>

<div
  class="work-canvas"
  bind:this={host}
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
      fitViewOptions={{ padding: 0.2, duration: 0 }}
      minZoom={0.2}
      maxZoom={2}
      nodeExtent={[
        [-1_000_000, -1_000_000],
        [1_000_000, 1_000_000],
      ]}
      proOptions={{ hideAttribution: true }}
      nodesConnectable={false}
      edgesFocusable={false}
      panOnScroll
      panOnScrollSpeed={1}
      zoomOnScroll={false}
      zoomOnPinch
      zoomOnDoubleClick={false}
      panOnDrag
      selectionOnDrag={false}
      autoPanOnNodeDrag
      elevateNodesOnSelect
      nodeDragThreshold={3}
      ariaLabelConfig={{ "edge.a11yDescription.default": m.work_env_edge_readonly() }}
      onlyRenderVisibleElements={items.length >= virtualizeFrom}
      elementsSelectable
      deleteKey={[]}
      onnodeclick={({ node, event }) => {
        if (
          event.target instanceof Element &&
          event.target.closest(".artifact-body, button, a, input, textarea, select, summary")
        )
          return;
        const now = performance.now();
        if (lastClick.id === node.id && now - lastClick.at < 320) {
          lastClick = { id: "", at: 0 };
          onopen?.(node.id);
          return;
        }
        lastClick = { id: node.id, at: now };
        oninspect(node.id);
      }}
      onpaneclick={() => {
        lastClick = { id: "", at: 0 };
        onselectionchange?.([]);
      }}
      onselectionchange={({ nodes: selected }) =>
        onselectionchange?.(selected.map((node) => node.id))}
      onmoveend={publishView}
    >
      <Background patternColor="var(--color-border)" gap={24} size={1} />
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
    min-height: 240px;

    --xy-edge-stroke-default: var(--color-border-strong);
    --xy-edge-stroke-width-default: 1.5;
    --xy-handle-background-color-default: transparent;
    --xy-handle-border-color-default: transparent;
    --xy-selection-background-color-default: var(--color-accent-soft);
    --xy-selection-border-default: 1px solid var(--color-accent);
    --xy-background-color-default: transparent;
  }

  .work-canvas :global(.svelte-flow) {
    min-height: 240px;
    background: transparent;
  }

  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__node) {
    transition: none;
  }

  .canvas-empty {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 24px;
    color: var(--color-muted);
  }
</style>

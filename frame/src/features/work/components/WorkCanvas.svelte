<script lang="ts">
  import "@xyflow/svelte/dist/base.css";
  import { SvelteFlow, Background, type Edge } from "@xyflow/svelte";
  import { untrack, setContext, tick } from "svelte";
  import {
    canvasInspection,
    canvasResize,
    canvasAction,
    canvasEvidence,
    canvasFocusResult,
    canvasOpen,
    canvasOpenLink,
    canvasAreas,
    canvasAuthor,
    canvasPictures,
  } from "../lib/canvas-context";
  import CanvasNode from "./CanvasNode.svelte";
  import AreaNode from "./AreaNode.svelte";
  import ClusterNode from "./ClusterNode.svelte";
  import CanvasControls from "./CanvasControls.svelte";
  import {
    reconcileNodes,
    applyRemoteView,
    absolutePosition,
    containingArea,
    isAreaNode,
    isItemNode,
    nodesBounds,
    sanitizeScene,
    validScene,
    validViewport,
    withClusters,
    edgeClass,
    type CanvasArea,
    type CanvasCluster,
    type CanvasItem,
    type CanvasLink,
    type CanvasPosition,
    type CanvasSize,
    type CanvasView,
    type WorkNode,
  } from "../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  let {
    items,
    links,
    clusters = [],
    areas = [],
    author = "",
    pictures = new Map(),
    initialView,
    remoteView,
    authoritative,
    fitBottomInset = 0,
    fitTopInset = 0,
    virtualizeFrom = 24,
    oninspect,
    onopen,
    onopenlink,
    onaction,
    onevidence,
    onviewchange,
    onselectionchange,
    onareachange,
    expose,
  }: {
    items: readonly CanvasItem[];
    links: readonly CanvasLink[];
    /** Each stage's cards of one kind; frontend-only, never persisted. */
    clusters?: readonly CanvasCluster[];
    areas?: readonly CanvasArea[];
    /** The person whose request starts a path on this canvas. */
    author?: string;
    /** The admitted picture of each subject, by merge key. */
    pictures?: ReadonlyMap<string, { profile: string; digest: string }>;
    initialView?: CanvasView;
    remoteView?: { sequence: number; view: CanvasView };
    authoritative: ReadonlySet<string>;
    fitBottomInset?: number;
    fitTopInset?: number;
    virtualizeFrom?: number;
    oninspect: (id: string) => void;
    onopen?: (id: string) => void;
    /** A prose link inside a result; opens through a native intent. */
    onopenlink?: (href: string) => void;
    onaction?: (id: string, action?: string) => void;
    onevidence?: (id: string, reference: EvidenceReference) => void;
    onviewchange?: (view: CanvasView) => void;
    onselectionchange?: (ids: string[]) => void;
    onareachange?: (id: string, area: string | null) => void;
    expose?: (api: CanvasApi) => void;
  } = $props();
  setContext(canvasEvidence, {
    get open() {
      return onevidence;
    },
  });
  setContext(canvasAction, (id: string, action?: string) => onaction?.(id, action));
  setContext(canvasOpenLink, (href: string) => onopenlink?.(href));
  setContext(canvasInspection, (id: string) => oninspect(id));
  setContext(canvasOpen, (id: string) => onopen?.(id));
  setContext(canvasAreas, {
    get list() {
      return areas;
    },
  });
  setContext(canvasAuthor, {
    get initial() {
      return author.slice(0, 1).toLocaleUpperCase();
    },
  });
  setContext(canvasPictures, {
    get map() {
      return pictures;
    },
  });
  let resizing = $state(false);
  setContext(canvasResize, (active: boolean) => (resizing = active));
  let nodes = $state.raw<WorkNode[]>([]);
  let canvasWidth = $state(0);
  let canvasHeight = $state(0);
  let host = $state<HTMLDivElement>();
  function center(id: string) {
    const node = nodes.find((node) => node.id === id);
    if (!node) return;
    const position = absolutePosition(node, nodes);
    viewport = {
      x: Math.max(24, (canvasWidth - (node.width ?? 480)) / 2) - position.x,
      y:
        fitTopInset +
        Math.max(0, (canvasHeight - fitBottomInset - fitTopInset - (node.height ?? 360)) / 2) -
        position.y,
      zoom: 1,
    };
    publishView();
  }
  setContext(canvasFocusResult, center);
  const restoredViewport = untrack(() => validViewport(initialView?.viewport));
  let viewport = $state(restoredViewport ?? { x: 0, y: 0, zoom: 1 });
  // One bad card never hides the canvas: the scene is repaired, then guarded.
  const scene = $derived(sanitizeScene(items, links, clusters));
  let valid = $derived(validScene(scene.items, scene.links, scene.clusters));
  const nodeTypes = { work: CanvasNode, area: AreaNode, cluster: ClusterNode };
  let selection: string[] = [];
  let selectedIds = $state.raw<ReadonlySet<string>>(new Set());
  let hovered = $state<string | null>(null);
  /** Cards whose ties show: the selection and the card under the pointer. */
  const focused = $derived<ReadonlySet<string>>(
    hovered ? new Set([...selectedIds, hovered]) : selectedIds,
  );
  /** When each path edge first appeared; only a new one draws itself in. */
  const firstSeen: Record<string, number> = {};
  let seeded = false;
  let edges = $derived<Edge[]>(
    valid
      ? scene.links.map((link) => {
          const active = focused.has(link.source) || focused.has(link.target);
          const title = (id: string) =>
            scene.items.find((item) => item.id === id)?.title ??
            scene.clusters.find((cluster) => cluster.id === id)?.label ??
            "";
          let draw = false;
          if (link.kind === "path" || link.kind === "thread") {
            const now = performance.now();
            firstSeen[link.id] ??= seeded ? now : -Infinity;
            draw = now - firstSeen[link.id]! < EDGE_DRAW_MS;
          }
          return {
            id: link.id,
            source: link.source,
            target: link.target,
            type: "smoothstep",
            animated: false,
            deletable: false,
            selectable: false,
            focusable: false,
            class: edgeClass(link, focused, draw),
            label: active && link.label ? link.label : undefined,
            ariaLabel:
              link.kind === "dependency" || link.kind === "path" || link.kind === "thread"
                ? m.work_env_dependency_label({
                    source: title(link.source),
                    target: title(link.target),
                  })
                : link.label
                  ? `${title(link.source)} ${link.label} ${title(link.target)}`
                  : m.work_env_reference_label({
                      source: title(link.source),
                      target: title(link.target),
                    }),
          };
        })
      : [],
  );
  const EDGE_DRAW_MS = 520;
  $effect(() => {
    // The first scene is already there when the canvas opens: nothing draws in.
    if (valid && scene.links.length) seeded = true;
  });
  $effect(() => {
    const next = scene.items;
    const grouping = areas;
    const ready = valid;
    nodes = untrack(() =>
      ready
        ? reconcileNodes(
            nodes,
            next,
            initialView?.positions,
            initialView?.sizes,
            grouping,
            initialView?.areas,
          )
        : [],
    );
  });
  $effect(() => {
    const groups = scene.clusters;
    const active = focused;
    const current = nodes;
    const next = withClusters(current, valid ? groups : [], active);
    if (next !== current) nodes = next;
  });
  let publishedPositions = "";
  const positionKey = (list: WorkNode[]) =>
    JSON.stringify(
      list
        .filter((node) => isItemNode(node) || isAreaNode(node))
        .map((node) => [
          node.id,
          node.parentId ?? "",
          node.position.x,
          node.position.y,
          node.width,
          node.height,
        ]),
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
    const elements = nodes.filter(isItemNode);
    const areaNodes = nodes.filter(isAreaNode);
    onviewchange?.({
      positions: Object.fromEntries(
        elements.map((node) => [node.id, absolutePosition(node, nodes)]),
      ),
      viewport: { ...viewport },
      sizes: Object.fromEntries(
        elements.map((node) => [
          node.id,
          { width: Math.round(node.width ?? 280), height: Math.round(node.height ?? 160) },
        ]),
      ),
      areas: Object.fromEntries(
        areaNodes.map((node) => [
          node.id.slice("area:".length),
          {
            x: Math.round(node.position.x),
            y: Math.round(node.position.y),
            width: Math.round(node.width ?? 640),
            height: Math.round(node.height ?? 420),
          },
        ]),
      ),
    });
  }
  type CanvasApi = {
    screenRect: (id: string) => DOMRect | null;
    flowPosition: (clientX: number, clientY: number) => CanvasPosition | null;
    selectionBounds: () => (CanvasPosition & CanvasSize & { ids: string[] }) | null;
    center: (id: string) => void;
    focusCard: (id: string) => void;
  };
  function selectionBounds() {
    const ids = selection.filter((id) => nodes.some((node) => node.id === id && isItemNode(node)));
    const bounds = nodesBounds(ids, nodes);
    return bounds ? { ...bounds, ids } : null;
  }
  function screenRect(id: string): DOMRect | null {
    const node = nodes.find((node) => node.id === id);
    const origin = host?.getBoundingClientRect();
    if (!node || !origin) return null;
    const z = viewport.zoom;
    const position = absolutePosition(node, nodes);
    return new DOMRect(
      origin.left + position.x * z + viewport.x,
      origin.top + position.y * z + viewport.y,
      (node.width ?? 280) * z,
      (node.height ?? 160) * z,
    );
  }
  /** Pans to a card and puts the keyboard on it; the node itself is the stop. */
  async function focusCard(id: string) {
    center(id);
    await tick();
    host
      ?.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${CSS.escape(id)}"]`)
      ?.focus({ preventScroll: true });
  }
  /** Where a client point lands on the canvas, so a drop keeps its place. */
  function flowPosition(clientX: number, clientY: number): CanvasPosition | null {
    const origin = host?.getBoundingClientRect();
    if (!origin) return null;
    return {
      x: (clientX - origin.left - viewport.x) / viewport.zoom,
      y: (clientY - origin.top - viewport.y) / viewport.zoom,
    };
  }
  $effect(() => {
    expose?.({
      screenRect,
      flowPosition,
      selectionBounds,
      center,
      focusCard: (id: string) => void focusCard(id),
    });
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
      fitView={scene.items.length > 0 && !restoredViewport}
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
      onlyRenderVisibleElements={scene.items.length >= virtualizeFrom}
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
      onnodepointerenter={({ node }) => {
        if (isItemNode(node as WorkNode)) hovered = node.id;
      }}
      onnodepointerleave={({ node }) => {
        if (hovered === node.id) hovered = null;
      }}
      onpaneclick={() => {
        lastClick = { id: "", at: 0 };
        onselectionchange?.([]);
      }}
      onselectionchange={({ nodes: selected }) => {
        selection = selected.map((node) => node.id);
        selectedIds = new Set(selection);
        onselectionchange?.(selection);
      }}
      onnodedragstop={({ targetNode }) => {
        if (!targetNode || !isItemNode(targetNode as WorkNode)) return;
        const node = nodes.find((candidate) => candidate.id === targetNode.id);
        if (!node || !isItemNode(node) || !authoritative.has(node.id)) return;
        const area = containingArea(node, nodes);
        const current = node.parentId ? node.parentId.slice("area:".length) : null;
        if (area !== current) onareachange?.(node.id, area);
      }}
      onmoveend={publishView}
    >
      <Background patternColor="var(--work-canvas-dot)" gap={20} size={1.5} />
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
    --work-canvas-dot: color-mix(in srgb, var(--color-text) 16%, transparent);
  }

  .work-canvas :global(.svelte-flow) {
    min-height: 240px;
    background: transparent;
  }

  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__node) {
    transition: none;
  }

  .work-canvas :global(.work-edge) {
    opacity: 0.45;
    transition: opacity var(--motion-fast) var(--ease-smooth);
  }

  .work-canvas :global(.work-edge.active) {
    opacity: 1;
  }

  /* A relation shows only while one of its ends is focused. */
  .work-canvas :global(.work-edge.latent) {
    opacity: 0;
    transition: opacity 140ms var(--ease-smooth);
  }

  .work-canvas :global(.work-edge.latent.active) {
    opacity: 1;
  }

  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__node.cluster-node) {
    pointer-events: none;
  }

  .work-canvas :global(.work-edge.kind-reference),
  .work-canvas :global(.work-edge.kind-uses),
  .work-canvas :global(.work-edge.kind-same-as) {
    stroke-dasharray: 5 5;
  }

  .work-canvas :global(.work-edge.kind-contradicts) {
    stroke: var(--color-danger);
  }

  /* A card that has just arrived settles in; it never bounces. */
  .work-canvas :global(.work-node-enter) {
    animation: work-node-in 260ms var(--ease-smooth);
  }

  @keyframes work-node-in {
    from {
      opacity: 0;
      translate: 0 5px;
    }

    to {
      opacity: 1;
      translate: 0 0;
    }
  }

  /* A path segment that just appeared strokes itself on from its stage. */
  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.work-edge.draw .svelte-flow__edge-path) {
    animation: work-edge-draw 520ms var(--ease-smooth);
  }

  @keyframes work-edge-draw {
    from {
      stroke-dasharray: 0 640;
    }

    to {
      stroke-dasharray: 640 0;
    }
  }

  /* An agent moves to its work; the tie to it is transient and alive. */
  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__node.agent-node) {
    transition: transform 700ms var(--ease-smooth);
  }

  .work-canvas :global(.work-edge.kind-working) {
    stroke: var(--color-accent);
    stroke-dasharray: 4 6;
    opacity: 0.9;
    animation: work-edge-flow 1.2s linear infinite;
  }

  @keyframes work-edge-flow {
    to {
      stroke-dashoffset: -20;
    }
  }

  .work-canvas :global(.work-edge.kind-supports.active) {
    stroke: var(--color-success);
  }

  .canvas-empty {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 24px;
    color: var(--color-muted);
  }
</style>

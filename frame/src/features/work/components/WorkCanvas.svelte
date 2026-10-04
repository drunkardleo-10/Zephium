<script lang="ts">
  import "@xyflow/svelte/dist/base.css";
  import { pointerTool as currentTool, setPointerTool } from "../lib/pointer-tool.svelte";
  const pointerTool = $derived(currentTool());
  import { SvelteFlow, Background, type Edge, type useSvelteFlow } from "@xyflow/svelte";
  import { NodeToolbar, Position, SelectionMode } from "@xyflow/svelte";
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
    canvasPictures,
    canvasAreaActions,
    canvasArrival,
    canvasProbe,
    canvasBoard,
    canvasSeen,
    canvasFar,
    canvasWork,
    type BoardActions,
  } from "../lib/canvas-context";
  import CanvasNode from "./CanvasNode.svelte";
  import AreaNode from "./AreaNode.svelte";
  import AgentMark from "./AgentMark.svelte";
  import WorkEdge from "./WorkEdge.svelte";
  import { arrivals } from "../lib/arrival";
  import { duration, easing, reducedMotion } from "$shared/lib/motion";
  import { IS_MAC } from "$shared/platform";
  import CanvasControls from "./CanvasControls.svelte";
  import SelectionBar from "./SelectionBar.svelte";
  import {
    alignTo,
    arrange,
    fitArea,
    moveTo,
    selectedPlacements,
    type Alignment,
    type Arrangement,
  } from "../lib/selection";
  import CanvasFlowHandle from "./CanvasFlowHandle.svelte";
  import {
    reconcileNodes,
    applyRemoteView,
    absolutePosition,
    containingArea,
    isAgentNode,
    isAreaNode,
    isItemNode,
    nodesBounds,
    sanitizeScene,
    validScene,
    validViewport,
    relationLinks,
    restLink,
    type CanvasArea,
    type CanvasItem,
    type CanvasLink,
    type CanvasPosition,
    type CanvasSize,
    type CanvasView,
    type WorkNode,
  } from "../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
  import { layerRelease } from "../lib/workspace/layers";
  let {
    items,
    links,
    areas = [],
    pictures = new Map(),
    initialView,
    remoteView,
    authoritative,
    fitBottomInset = 0,
    fitTopInset = 0,
    home,
    still = false,
    virtualizeFrom = 24,
    oninspect,
    onopen,
    onopenlink,
    onaction,
    onevidence,
    onviewchange,
    onselectionchange,
    onareachange,
    onselectionaction,
    onareaedit,
    onmoved,
    board,
    work,
    onprobe,
    expose,
  }: {
    items: readonly CanvasItem[];
    links: readonly CanvasLink[];
    areas?: readonly CanvasArea[];
    /** The admitted picture of each subject, by merge key. */
    pictures?: ReadonlyMap<string, { profile: string; digest: string }>;
    /** Without a camera, the canvas opens on its home run. */
    initialView?: Omit<CanvasView, "viewport"> & { viewport?: CanvasView["viewport"] };
    remoteView?: { sequence: number; view: CanvasView };
    authoritative: ReadonlySet<string>;
    fitBottomInset?: number;
    fitTopInset?: number;
    /** The run a canvas with no saved camera opens on: framed to its width, its result whole. */
    home?: { x: number; y: number; width: number };
    /** A lift, the pane or the takeover is open: the camera does not follow the agent. */
    still?: boolean;
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
    /** A selection bar action on the selected cards. */
    onselectionaction?: (action: "area" | "ask" | "remove", ids: string[]) => void;
    onareaedit?: (
      area: string,
      edit: { kind: "rename"; title: string } | { kind: "remove" },
    ) => void;
    /** The person dragged these cards to where they stand now. */
    onmoved?: (ids: string[]) => void;
    /** What a board's blocks ask of the canvas's owner. */
    board?: BoardActions;
    /** The runs behind the canvas, by objective, for a helper's own view of its part. */
    work?: (objective: string) => WorkRuntimeProjection | undefined;
    /** Asks native for the icon of an origin no tab has shown. */
    onprobe?: (origin: string) => void;
    expose?: (api: CanvasApi) => void;
  } = $props();
  setContext(canvasWork, (objective: string) => work?.(objective));
  setContext(canvasBoard, {
    measure: (id: string, width: number, open: boolean, height: number) =>
      board?.measure(id, width, open, height),
    toggle: (id: string) => board?.toggle(id),
    ask: (name: string) => board?.ask(name),
    choose: (element: string, chosen: boolean) => board?.choose(element, chosen),
    evidence: (reference: EvidenceReference) => board?.evidence(reference),
    entity: (element: string) => board?.entity(element),
    command: (record: string) => board?.command(record),
    page: (url: string) => board?.page(url),
    note: (id: string) => board?.note(id),
    compare: (id: string) => board?.compare?.(id),
    send: (id: string) => board?.send?.(id),
    write: (id: string, markdown: string) => board?.write?.(id, markdown),
  } satisfies BoardActions);
  /** Each origin is asked for once per canvas. */
  const probed: Record<string, true> = {};
  setContext(canvasProbe, (origin: string) => {
    if (probed[origin]) return;
    probed[origin] = true;
    onprobe?.(origin);
  });
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
  let layers: ReturnType<typeof layerRelease> | undefined;
  $effect(() => {
    const root = host;
    if (!root) return;
    const release = layerRelease(root);
    layers = release;
    return () => {
      release.destroy();
      if (layers === release) layers = undefined;
    };
  });
  // Whatever arrives, leaves or settles on the canvas moves: the layer is let go once it is still.
  $effect(() => {
    void nodes;
    void edges;
    untrack(() => layers?.moved());
  });
  const nodeFor = (id: string) => nodes.find((node) => node.id === id);
  function center(id: string) {
    const node = nodeFor(id);
    if (!node) return;
    homed = false;
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
  const AIR = 64;
  const HOME_SETTLE = 1500;
  /**
   * The view that holds a span of the canvas across its width: at 100% when
   * it fits, down to 50%; wider still, its right end stays in view, so a
   * run's result is never cut.
   */
  function framing(rect: { x: number; y: number; width: number }) {
    const room = Math.max(1, canvasWidth - AIR * 2);
    const zoom = Math.max(0.5, Math.min(1, room / Math.max(1, rect.width)));
    const over = Math.max(0, rect.width * zoom - room);
    return { x: AIR - rect.x * zoom - over, y: fitTopInset + 40 - rect.y * zoom, zoom };
  }
  /** Brings a card to the reading place: its corner near the top left, as large as its span lets it read whole. */
  function reveal(id: string, span?: number, tries = 30) {
    const node = nodeFor(id);
    // The answer lands a frame or two after its run ends: wait for it rather than for nothing.
    if (!node && tries > 0) {
      requestAnimationFrame(() => reveal(id, span, tries - 1));
      return;
    }
    if (!node || !flow) return;
    homed = false;
    const position = absolutePosition(node, nodes);
    void flow.setViewport(framing({ ...position, width: span ?? node.width ?? 480 }), {
      duration: reducedMotion() ? 0 : duration("page"),
      ease: curve(easing("emphasized")),
      interpolate: "linear",
    });
  }
  const restoredViewport = untrack(() => validViewport(initialView?.viewport));
  let viewport = $state(restoredViewport ?? { x: 0, y: 0, zoom: 1 });
  /**
   * A canvas with no saved camera stands on its home run, and keeps standing
   * there as the run's objects measure themselves, until the person moves.
   */
  let homed = !restoredViewport;
  $effect(() => {
    const target = home;
    if (!homed || !target || !canvasWidth) return;
    untrack(() => (viewport = framing(target)));
  });
  // Homing lasts while the opened run measures itself, never through a run that goes on growing.
  $effect(() => {
    if (!homed || !canvasWidth) return;
    const settle = setTimeout(() => (homed = false), HOME_SETTLE);
    return () => clearTimeout(settle);
  });
  /**
   * The nodes that have come within a screen of the view. Until one has, it
   * is an empty box of its size: a large work opens drawing what can be seen,
   * not every object once before the flow culls it. Once near, a node stays
   * drawn while the flow keeps it; the set only grows, and only changes when
   * something new comes near.
   */
  let near = $state.raw<ReadonlySet<string>>(new Set());
  $effect(() => {
    const view = viewport;
    // Before the canvas has measured itself, the window stands in for it.
    const width = canvasWidth || window.innerWidth;
    const height = canvasHeight || window.innerHeight;
    const list = nodes;
    untrack(() => {
      const left = -view.x / view.zoom - width / view.zoom;
      const top = -view.y / view.zoom - height / view.zoom;
      const right = left + (width * 3) / view.zoom;
      const bottom = top + (height * 3) / view.zoom;
      const added: string[] = [];
      for (const node of list) {
        if (near.has(node.id)) continue;
        const at = absolutePosition(node, list);
        const w = node.width ?? node.measured?.width ?? 0;
        const h = node.height ?? node.measured?.height ?? 0;
        if (at.x > right || at.y > bottom || at.x + w < left || at.y + h < top) continue;
        added.push(node.id);
      }
      if (added.length) near = new Set([...near, ...added]);
    });
  });
  setContext(canvasSeen, (id: string) => near.has(id));
  /** Seen from far, with a margin either way so a pinch around the line doesn't flicker. */
  let far = $state(false);
  $effect(() => {
    const zoom = viewport.zoom;
    untrack(() => {
      if (!far && zoom < 0.5) far = true;
      else if (far && zoom > 0.6) far = false;
    });
  });
  setContext(canvasFar, {
    get far() {
      return far;
    },
  });
  // One bad card never hides the canvas: the scene is repaired, then guarded.
  const scene = $derived(sanitizeScene(items, links));
  let valid = $derived(validScene(scene.items, scene.links));
  const nodeTypes = { work: CanvasNode, area: AreaNode, agent: AgentMark };
  const edgeTypes = { work: WorkEdge };
  let selection: string[] = [];
  let selectedIds = $state.raw<ReadonlySet<string>>(new Set());
  let hovered = $state<string | null>(null);
  /** Cards whose ties show: the selection and the card under the pointer. */
  const focused = $derived<ReadonlySet<string>>(
    hovered ? new Set([...selectedIds, hovered]) : selectedIds,
  );
  /** What arrived when: only what a live run adds after the canvas opened moves in. */
  const arrival = arrivals();
  setContext(canvasArrival, (id: string) => arrival.motion(id));
  const byId = $derived(new Map(scene.items.map((item) => [item.id, item])));
  const lit = $derived(relationLinks(scene.links, byId, focused));
  const title = (id: string) => byId.get(id)?.title ?? "";

  let edges = $derived.by<Edge[]>(() => {
    if (!valid) return [];
    arrival.see(scene.items);
    return scene.links.flatMap((link) => {
      if (link.hover && !focused.has(link.source) && !focused.has(link.target)) return [];
      const rest = restLink(link);
      if (!rest && !lit.has(link.id)) return [];
      return [
        {
          id: link.id,
          source: link.source,
          target: link.target,
          type: "work",
          data: {
            ...(link.route ? { route: link.route } : {}),
            tone:
              link.kind === "thread"
                ? "thread"
                : link.kind === "flow"
                  ? "flow"
                  : rest
                    ? "rest"
                    : "relation",
            ...(link.live ? { live: true } : {}),
          },
          deletable: false,
          selectable: false,
          focusable: false,
          class: `work-edge kind-${link.kind}`,
          ariaLabel:
            link.kind === "dependency" || rest
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
        },
      ];
    });
  });
  /** The positions the last reconcile was given: a card follows only a place that moved. */
  let lastPositions: Readonly<Record<string, CanvasPosition>> = {};
  /** An agent whose run ended rests where it finished, then fades. */
  let resting: WorkNode[] = [];
  const REST_MS = 1200;
  function rest(previous: readonly WorkNode[], next: WorkNode[]): WorkNode[] {
    const ids = new Set(next.map((node) => node.id));
    resting = resting.filter((node) => !ids.has(node.id));
    for (const node of previous) {
      if (!isAgentNode(node) || ids.has(node.id) || resting.some((r) => r.id === node.id)) continue;
      const still = {
        ...node,
        data: { ...node.data, agent: { ...node.data.agent!, caption: undefined } },
        class: "resting",
      };
      resting.push(still);
      setTimeout(() => {
        resting = resting.map((r) => (r.id === node.id ? { ...r, class: "resting leaving" } : r));
        nodes = nodes.map((candidate) =>
          candidate.id === node.id ? { ...candidate, class: "resting leaving" } : candidate,
        );
        setTimeout(() => {
          resting = resting.filter((r) => r.id !== node.id);
          nodes = nodes.filter((candidate) => candidate.id !== node.id);
        }, duration("slow"));
      }, REST_MS);
    }
    if (!resting.length) return next;
    const kept = [...next, ...resting.filter((r) => !ids.has(r.id))];
    return kept.length === previous.length && kept.every((node, index) => node === previous[index])
      ? (previous as WorkNode[])
      : kept;
  }
  $effect(() => {
    const next = scene.items;
    const grouping = areas;
    const ready = valid;
    const placed = initialView?.positions ?? {};
    nodes = untrack(() => {
      if (!ready) return [];
      arrival.see(next);
      const reconciled = reconcileNodes(
        nodes,
        next,
        placed,
        initialView?.sizes,
        grouping,
        { ...initialView?.areas, ...pendingAreas },
        lastPositions,
      );
      lastPositions = placed;
      return rest(nodes, reconciled);
    });
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
    selectionBounds: (
      only?: readonly string[],
    ) => (CanvasPosition & CanvasSize & { ids: string[] }) | null;
    placeArea: (area: string, rect: CanvasPosition & CanvasSize) => void;
    center: (id: string) => void;
    focusCard: (id: string) => void;
    reveal: (id: string, span?: number) => void;
    followAgent: () => boolean;
    resumeFollow: () => void;
  };
  function selectionBounds(only?: readonly string[]) {
    const ids = (only ?? selection).filter((id) =>
      nodes.some((node) => node.id === id && isItemNode(node)),
    );
    const bounds = nodesBounds(ids, nodes);
    return bounds ? { ...bounds, ids } : null;
  }
  function screenRect(id: string): DOMRect | null {
    const node = nodeFor(id);
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
      placeArea,
      center,
      focusCard: (id: string) => void focusCard(id),
      reveal,
      followAgent: () => following,
      resumeFollow,
    });
  });

  // Camera follow. While a run is live and its mark walks to a stand out of
  // sight, the view pans (never zooms) so the mark lands in the lower-right
  // third. A manual pan, zoom or drag pauses it until a new lane or a new run.
  const FOLLOW_AIR = 24;
  /** The orb and the widest caption it usually carries. */
  const MARK_REACH = { width: 220, height: 24 };
  let flow: ReturnType<typeof useSvelteFlow> | undefined;
  let following = $state(true);
  let fitted = false;
  let requestsSeen = false;
  let stands: Record<string, string> = {};
  const requests: Record<string, true> = {};
  function resumeFollow() {
    following = true;
  }
  /** A `cubic-bezier()` token as a function of time, for the flow's own tween. */
  function curve(token: string): (t: number) => number {
    const [a, b, c, d] = (token.match(/-?[\d.]+/gu) ?? []).map(Number);
    if ([a, b, c, d].some((value) => value === undefined || Number.isNaN(value))) return (t) => t;
    const bezier = (p: number, q: number, u: number) =>
      3 * p * u * (1 - u) ** 2 + 3 * q * u ** 2 * (1 - u) + u ** 3;
    return (t) => {
      let low = 0;
      let high = 1;
      for (let step = 0; step < 24; step++) {
        const mid = (low + high) / 2;
        if (bezier(a!, c!, mid) < t) low = mid;
        else high = mid;
      }
      return bezier(b!, d!, (low + high) / 2);
    };
  }
  /**
   * A request just sent stands in the middle of what can be seen: between
   * the island and the composer, at the zoom the person reads at.
   */
  function centreRequest(id: string, tries = 12) {
    const node = nodeFor(id);
    if (!node || !flow || !canvasWidth || !(node.width ?? node.measured?.width)) {
      // A just-sent card is placed and measured a few frames after it exists.
      if (tries > 0) requestAnimationFrame(() => centreRequest(id, tries - 1));
      return;
    }
    homed = false;
    const zoom = viewport.zoom;
    const position = absolutePosition(node, nodes);
    const width = (node.width ?? node.measured?.width ?? 320) * zoom;
    const height = (node.height ?? node.measured?.height ?? 80) * zoom;
    const top = fitTopInset;
    const bottom = canvasHeight - fitBottomInset;
    void flow.setViewport(
      {
        x: (canvasWidth - width) / 2 - position.x * zoom,
        y: top + (bottom - top - height) / 2 - position.y * zoom,
        zoom,
      },
      {
        duration: reducedMotion() ? 0 : duration("page"),
        ease: curve(easing("emphasized")),
        interpolate: "linear",
      },
    );
  }
  function followTo(position: CanvasPosition, size: CanvasSize) {
    const zoom = viewport.zoom;
    const top = fitTopInset;
    const bottom = canvasHeight - fitBottomInset;
    const width = canvasWidth;
    const air = FOLLOW_AIR * zoom;
    const box = {
      left: position.x * zoom + viewport.x - air,
      top: position.y * zoom + viewport.y - air,
      right: (position.x + size.width) * zoom + viewport.x + air,
      bottom: (position.y + size.height) * zoom + viewport.y + air,
    };
    if (box.left >= 0 && box.top >= top && box.right <= width && box.bottom <= bottom) return;
    const w = size.width * zoom;
    const h = size.height * zoom;
    const clamp = (value: number, min: number, max: number) =>
      Math.max(min, Math.min(value, Math.max(min, max)));
    const x = clamp((width * 2) / 3 - w / 2, air, width - air - w);
    const y = clamp(top + (bottom - top) / 2 - h / 2, top + air, bottom - air - h);
    void flow?.setViewport(
      { x: x - position.x * zoom, y: y - position.y * zoom, zoom },
      {
        duration: reducedMotion() ? 0 : duration("page"),
        ease: curve(easing("smooth")),
        interpolate: "linear",
      },
    );
  }
  $effect(() => {
    const agents = nodes.filter(isAgentNode).filter((node) => !node.class);
    const cards = scene.items.flatMap((item) =>
      item.type === "request" || item.type === "objective" ? [item.id] : [],
    );
    const paused = still;
    untrack(() => {
      // A new lane's request card, or the end of a run, resumes following.
      // Cards present when the canvas first looks (or arriving together, as a
      // work loads) are not new; one card appearing on its own was just sent,
      // including the first request of an empty work.
      const fresh = cards.filter((id) => !requests[id]);
      for (const id of fresh) requests[id] = true;
      const sent = requestsSeen && fresh.length === 1 ? fresh[0] : undefined;
      requestsSeen = true;
      if (sent) {
        following = true;
        requestAnimationFrame(() => centreRequest(sent));
      }
      if (!agents.length && Object.keys(stands).length) {
        stands = {};
        following = true;
      }
      for (const node of agents) {
        const position = node.position;
        const key = `${position.x},${position.y}`;
        const moved = stands[node.id] !== key;
        stands[node.id] = key;
        if (!moved || !fitted || !following || paused) continue;
        followTo(position, MARK_REACH);
      }
    });
  });
  let lastClick = { id: "", at: 0 };
  // Selection, marquee, arrange and areas.
  const pendingAreas: Record<string, CanvasPosition & CanvasSize> = {};
  /** Where a new area stands: kept until its node exists, or moved in place once it does. */
  function placeArea(area: string, rect: CanvasPosition & CanvasSize) {
    const id = `area:${area}`;
    if (!nodes.some((node) => node.id === id)) {
      pendingAreas[area] = rect;
      return;
    }
    nodes = nodes.map((node) =>
      node.id === id
        ? { ...node, position: { x: rect.x, y: rect.y }, width: rect.width, height: rect.height }
        : node,
    );
  }
  let marquee = $state(false);
  let engaged = false;
  const selectedItems = $derived(
    nodes.flatMap((node) => (node.selected && isItemNode(node) ? [node.id] : [])),
  );
  const dragging = $derived(nodes.some((node) => node.dragging));
  function place(next: WorkNode[]) {
    if (next === nodes) return;
    nodes = next;
    publishedPositions = positionKey(next);
    publishView();
  }
  const arrangeSelection = (how: Arrangement) =>
    place(moveTo(nodes, arrange(selectedPlacements(selectedItems, nodes), how)));
  const alignSelection = (how: Alignment) =>
    place(moveTo(nodes, alignTo(selectedPlacements(selectedItems, nodes), how)));
  function clearSelection() {
    if (!nodes.some((node) => node.selected)) return false;
    nodes = nodes.map((node) => (node.selected ? { ...node, selected: false } : node));
    return true;
  }
  setContext(canvasAreaActions, {
    fit: (id: string) => place(fitArea(nodes, id)),
    rename: (id: string, title: string) =>
      onareaedit?.(id.slice("area:".length), { kind: "rename", title }),
    remove: (id: string) => onareaedit?.(id.slice("area:".length), { kind: "remove" }),
  });
  /** V and H pick the tool, Escape clears the selection, while the canvas holds the keyboard. */
  function canvasKeys(event: KeyboardEvent) {
    if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
    const active = document.activeElement;
    const inside = !!active && !!host?.contains(active);
    if (!inside && !(engaged && (!active || active === document.body))) return;
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (target?.closest("input, textarea, select, [contenteditable]")) return;
    const key = event.key.toLowerCase();
    if (key === "v" || key === "h") {
      setPointerTool(key === "v" ? "select" : "hand");
      event.preventDefault();
    } else if (event.key === "Escape" && clearSelection()) event.preventDefault();
  }
</script>

<svelte:window
  onpointerdowncapture={(event) => (engaged = !!host?.contains(event.target as Node))}
  onkeydown={canvasKeys}
/>

<div
  class="work-canvas"
  class:multi={selectedItems.length > 1}
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
      {edgeTypes}
      bind:viewport
      fitView={scene.items.length > 0 && !restoredViewport && !home}
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
      panOnDrag={pointerTool === "hand"}
      selectionOnDrag={pointerTool === "select"}
      selectionKey="Shift"
      multiSelectionKey={[IS_MAC ? "Meta" : "Control", "Shift"]}
      panActivationKey=" "
      selectionMode={SelectionMode.Partial}
      onselectionstart={() => (marquee = true)}
      onselectionend={() => {
        marquee = false;
        // A marquee that caught cards leaves the areas it touched alone.
        if (selectedItems.length && nodes.some((node) => node.selected && isAreaNode(node)))
          nodes = nodes.map((node) =>
            node.selected && isAreaNode(node) ? { ...node, selected: false } : node,
          );
      }}
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
          // An input that stands for something on the canvas opens that thing.
          const input = isItemNode(node as WorkNode) ? (node.data as CanvasItem).input : undefined;
          onopen?.(input?.element ?? node.id);
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
      onnodedragstop={({ targetNode, nodes: carried }) => {
        onmoved?.(carried.map((node) => node.id));
        if (!targetNode || !isItemNode(targetNode as WorkNode)) return;
        const node = nodes.find((candidate) => candidate.id === targetNode.id);
        if (!node || !isItemNode(node) || !authoritative.has(node.id)) return;
        const area = containingArea(node, nodes);
        const current = node.parentId ? node.parentId.slice("area:".length) : null;
        if (area !== current) onareachange?.(node.id, area);
      }}
      onmoveend={publishView}
      oninit={() => requestAnimationFrame(() => requestAnimationFrame(() => (fitted = true)))}
      onmovestart={(event) => {
        if (!event) return;
        following = false;
        homed = false;
      }}
      onnodedragstart={() => {
        following = false;
        homed = false;
      }}
    >
      <CanvasFlowHandle onready={(handle) => (flow = handle)} />
      <Background patternColor="var(--work-canvas-dot)" gap={20} size={1.5} />
      <CanvasControls bottomInset={fitBottomInset} />
      <NodeToolbar
        nodeId={selectedItems}
        isVisible={selectedItems.length > 1 && !dragging && !marquee}
        position={Position.Top}
        offset={14}
      >
        <SelectionBar
          count={selectedItems.length}
          owned={selectedItems.filter((id) => authoritative.has(id)).length}
          onarrange={arrangeSelection}
          onalign={alignSelection}
          onarea={() => onselectionaction?.("area", selectedItems)}
          onask={() => onselectionaction?.("ask", selectedItems)}
          onremove={() => onselectionaction?.("remove", selectedItems)}
        />
      </NodeToolbar>
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

  /* A handle is where a line meets a card, never a thing to see. */
  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__handle) {
    opacity: 0;
  }

  /* A board's block glides where its board makes room; a dragged one follows the pointer. */
  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__node.board-block:not(.dragging)) {
    transition: transform var(--motion-slow) var(--ease-emphasized);
  }

  /* A part's windows fan out over what follows its row. */
  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__node:has(.fans:hover)) {
    z-index: 1000 !important;
  }

  /* The agent's mark walks to its work on a spring; it never takes the pointer. */
  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__node-agent) {
    pointer-events: none;
    transition: opacity var(--motion-slow) var(--ease-exit);
  }

  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__node-agent.leaving) {
    opacity: 0;
  }

  /* Several cards selected: one bar for all of them, not a toolbar and handles each. */
  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas.multi :global(.svelte-flow__node-toolbar:not(:has(.selection-bar))),
  .work-canvas.multi :global(.work-resize-handle) {
    display: none;
  }

  /* An area is taken by its title; its body lets the pane pan or draw a marquee. */
  /* stylelint-disable-next-line selector-class-pattern */
  .work-canvas :global(.svelte-flow__node-area) {
    pointer-events: none;
  }

  /* stylelint-disable selector-class-pattern */
  .work-canvas :global(.svelte-flow__node-area .area-title),
  .work-canvas :global(.svelte-flow__node-area .work-resize-handle) {
    pointer-events: auto;
  }
  /* stylelint-enable selector-class-pattern */

  .canvas-empty {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 24px;
    color: var(--color-muted);
  }
</style>

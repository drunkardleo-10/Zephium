import type { MediaAssetV1 } from "$domain/resources";
import type { ArtifactView, FindingView, SubjectView } from "$shared/ui/data/Artifact";
import type { Node } from "@xyflow/svelte";
import type { HumanPage } from "./work-human";

type CanvasKind =
  | "tab"
  | "note"
  | "media"
  | "objective"
  | "request"
  | "responsibility"
  | "result"
  | "subject"
  | "finding"
  | "sources"
  | "folder"
  | "link"
  | "page"
  | "agent"
  | "findings"
  | "file"
  | "command";
type RelationKind = "supports" | "uses" | "depends_on" | "same_as" | "contradicts";
/** Display values only; deliberately independent from the generated Work wire contract. */
export type CanvasItem = {
  id: string;
  title: string;
  kind: string;
  type?: CanvasKind;
  detail: string;
  status: string;
  area?: string | null;
  /** Fixed-raster native favicon only; never a remote image URL. */
  favicon?: string | null;
  artifact?: ArtifactView;
  layout?: "artifact";
  actionLabel?: string;
  /** Planned output names only; these are not produced artifact resources. */
  responsibility?: { outputs: string[] };
  subject?: SubjectView;
  /** What the run established about a subject, price first. */
  facts?: { label: string; value: string }[];
  finding?: FindingView;
  /** One findings artifact folded into one card: its claims, most confident first. */
  findings?: {
    items: {
      claim: string;
      confidence: FindingView["confidence"];
      subject?: string;
      evidence: number;
    }[];
    total: number;
  };
  /** A file the run touched, one card per distinct path per run. */
  file?: {
    name: string;
    folder: string;
    what: "read" | "searched" | "changed" | "created";
    delta?: { plus: number; minus: number };
  };
  /** A command the run executed; the tail is its last output lines. */
  command?: {
    line: string;
    state: "running" | "exit";
    exit?: number;
    elapsed_ms?: number;
    tail: string[];
    reason?: string;
    /** The settled command's record id; the lift opens it whole. */
    record?: string;
  };
  /** Transient agent presence: avatar seed, status, its latest line, and where it stands. */
  agent?: {
    seed: number;
    activity: string;
    objective: string;
    line?: string;
    doing?: string;
    stand?: CanvasPosition;
  };
  /** The pages one fetch stage cited; opening a row goes through the pane. */
  sources?: readonly {
    key: string;
    /** Empty for a file: a granted folder is not a place the pane can open. */
    url: string;
    where: string;
    title: string;
    /** Why the read gave up, when it did; the row says so instead of the title. */
    note?: string;
    /** A file a step disclosed; the lift shows what the run recorded of it. */
    file?: { record: string; path: string; kind: string };
  }[];
  /** A page a browser step opened: its newest frame while the agent works there. */
  page?: {
    url: string;
    host: string;
    frame: string | null;
    live: boolean;
    /** Set while the run is holding this page open for a person. */
    human?: HumanPage;
  };
  unavailable?: boolean;
  /** The stage a run is working in right now; it glows while that is true. */
  active?: boolean;
  /** The user's recorded choice about this element. */
  decision?: string;
  /** An admitted media asset; the image URL is derived from profile and digest. */
  media?: { profile: string; asset: MediaAssetV1 };
  /** An admitted image related to this element, shown as its picture. */
  image?: { profile: string; digest: string };
};
export type CanvasLink = {
  id: string;
  source: string;
  target: string;
  /**
   * `path` reads through a stage at rest and `thread` joins one stage to the
   * next; relation kinds show only while an end is selected or hovered;
   * `working` is the transient tie between an agent and what it acts on now.
   */
  kind: "dependency" | "reference" | "working" | "path" | "thread" | RelationKind;
  label?: string;
};
/** What Rust relates and the run's own ties: drawn only while an end is focused. */
const LATENT = new Set<CanvasLink["kind"]>([
  "supports",
  "uses",
  "depends_on",
  "same_as",
  "contradicts",
]);
/** The path is drawn at rest; relations wait for a focused end; a new path segment draws in. */
export function edgeClass(link: CanvasLink, focused: ReadonlySet<string>, fresh: boolean): string {
  const active = focused.has(link.source) || focused.has(link.target);
  const path = link.kind === "path" || link.kind === "thread";
  return [
    "work-edge",
    `kind-${link.kind}`,
    ...(LATENT.has(link.kind) ? ["latent"] : []),
    ...(active ? ["active"] : []),
    ...(path && fresh ? ["draw"] : []),
  ].join(" ");
}
export type CanvasPosition = { x: number; y: number };
export type CanvasSize = { width: number; height: number };
export type CanvasArea = { id: string; title: string };
export type CanvasView = {
  positions: Record<string, CanvasPosition>;
  sizes?: Record<string, CanvasSize>;
  areas?: Record<string, CanvasPosition & CanvasSize>;
  viewport: { x: number; y: number; zoom: number };
};
export type AreaData = { title: string; count: number };
/** The cards of one kind in one stage; derived by projection, never persisted. */
export type CanvasCluster = { id: string; label: string; more: number; members: readonly string[] };
export type ClusterData = { label: string; more: number; active: boolean };
export type WorkItemNode = Node<CanvasItem, "work">;
type ClusterNode = Node<ClusterData, "cluster">;
export type WorkNode = WorkItemNode | Node<AreaData, "area"> | ClusterNode;
const AREA_PREFIX = "area:";
const areaNodeId = (id: string) => `${AREA_PREFIX}${id}`;
export const isAreaNode = (node: WorkNode): node is Node<AreaData, "area"> => node.type === "area";
const isClusterNode = (node: WorkNode): node is ClusterNode => node.type === "cluster";
export const isItemNode = (node: WorkNode): node is WorkItemNode => node.type === "work";
const DEFAULT_AREA: CanvasSize = { width: 640, height: 420 };
const CLUSTER_PAD = 12;
const validPosition = (p: CanvasPosition | undefined): p is CanvasPosition =>
  !!p &&
  Number.isFinite(p.x) &&
  Number.isFinite(p.y) &&
  Math.abs(p.x) <= 1_000_000 &&
  Math.abs(p.y) <= 1_000_000;
const validSize = (s: CanvasSize | undefined): s is CanvasSize =>
  !!s &&
  Number.isInteger(s.width) &&
  Number.isInteger(s.height) &&
  s.width >= 120 &&
  s.width <= 4096 &&
  s.height >= 80 &&
  s.height <= 4096;
const validAreaSize = (s: CanvasSize | undefined): s is CanvasSize =>
  !!s &&
  Number.isInteger(s.width) &&
  Number.isInteger(s.height) &&
  s.width >= 240 &&
  s.width <= 8192 &&
  s.height >= 160 &&
  s.height <= 8192;
const CANVAS_ITEM_LIMIT = 500;
export function defaultSize(item: CanvasItem): { width: number; height: number } {
  if (item.type === "findings") return { width: 300, height: 200 };
  if (item.artifact) {
    switch (item.artifact.content.kind) {
      case "comparison":
      case "matrix":
        return { width: 520, height: 320 };
      case "sources":
        return { width: 320, height: 240 };
      case "browser":
        return { width: 320, height: 180 };
      default:
        return { width: 420, height: 300 };
    }
  }
  switch (item.type) {
    case "tab":
    case "link":
      return { width: 280, height: 96 };
    case "subject":
      return { width: 220, height: item.image ? 248 : 136 };
    case "finding":
      return { width: 300, height: 140 };
    case "sources":
      return { width: 300, height: 200 };
    case "folder":
    case "file":
      return { width: 248, height: 96 };
    case "command":
      return { width: 248, height: 120 };
    case "note":
      return { width: 300, height: 200 };
    case "media":
      return { width: 248, height: 200 };
    case "objective":
    case "request":
      return { width: 300, height: 110 };
    case "responsibility":
      return { width: 280, height: 150 };
    case "page":
      return { width: 248, height: 168 };
    case "agent":
      return { width: 260, height: item.agent?.line ? 104 : 84 };
    default:
      return { width: 280, height: 160 };
  }
}
const CANVAS_LINK_LIMIT = 2000;
const TEXT_LIMIT = { id: 128, title: 512, detail: 2048, kind: 128, status: 256 } as const;

/** Clips to a whole character: a cut never splits a surrogate pair. */
export function clipText(value: string, max: number): string {
  if (value.length <= max) return value;
  const cut = value.slice(0, max);
  const last = cut.charCodeAt(max - 1);
  return last >= 0xd800 && last <= 0xdbff ? cut.slice(0, -1) : cut;
}

/**
 * Whatever a producer hands over becomes a scene the canvas can draw: long text
 * is clipped, repeated and empty identities are dropped, links that lead
 * nowhere are left out, and the counts stop at the limits. One bad item costs
 * that item, never the canvas.
 */
export function sanitizeScene(
  items: readonly CanvasItem[],
  links: readonly CanvasLink[],
  clusters: readonly CanvasCluster[] = [],
): { items: CanvasItem[]; links: CanvasLink[]; clusters: CanvasCluster[] } {
  const ids = new Set<string>();
  const kept: CanvasItem[] = [];
  for (const item of items) {
    if (kept.length >= CANVAS_ITEM_LIMIT) break;
    if (!item.id || item.id.length > TEXT_LIMIT.id || ids.has(item.id)) continue;
    ids.add(item.id);
    const title = clipText(item.title, TEXT_LIMIT.title);
    const detail = clipText(item.detail, TEXT_LIMIT.detail);
    const kind = clipText(item.kind, TEXT_LIMIT.kind);
    const status = clipText(item.status, TEXT_LIMIT.status);
    kept.push(
      title === item.title && detail === item.detail && kind === item.kind && status === item.status
        ? item
        : { ...item, title, detail, kind, status },
    );
  }
  // Clusters count against no limit: they are added once the cards are settled.
  const groups: CanvasCluster[] = [];
  const endpoints = new Set(ids);
  for (const cluster of clusters) {
    if (!cluster.id || cluster.id.length > TEXT_LIMIT.id || endpoints.has(cluster.id)) continue;
    const members = cluster.members.filter((member) => ids.has(member));
    if (!members.length) continue;
    endpoints.add(cluster.id);
    groups.push({
      ...cluster,
      label: clipText(cluster.label, TEXT_LIMIT.title),
      members,
    });
  }
  const seen = new Set<string>();
  const edges: CanvasLink[] = [];
  for (const link of links) {
    if (edges.length >= CANVAS_LINK_LIMIT) break;
    if (!link.id || link.id.length > TEXT_LIMIT.id || seen.has(link.id)) continue;
    if (link.source === link.target || !endpoints.has(link.source) || !endpoints.has(link.target))
      continue;
    seen.add(link.id);
    edges.push(link);
  }
  return { items: kept, links: edges, clusters: groups };
}

export function validScene(
  items: readonly CanvasItem[],
  links: readonly CanvasLink[],
  clusters: readonly CanvasCluster[] = [],
): boolean {
  if (items.length > CANVAS_ITEM_LIMIT || links.length > CANVAS_LINK_LIMIT) return false;
  const ids = new Set([...items.map((item) => item.id), ...clusters.map((cluster) => cluster.id)]);
  return (
    ids.size === items.length + clusters.length &&
    items.every(
      (item) =>
        item.id.length > 0 &&
        item.id.length <= TEXT_LIMIT.id &&
        item.title.length <= TEXT_LIMIT.title &&
        item.detail.length <= TEXT_LIMIT.detail &&
        item.kind.length <= TEXT_LIMIT.kind &&
        item.status.length <= TEXT_LIMIT.status,
    ) &&
    new Set(links.map((link) => link.id)).size === links.length &&
    links.every(
      (link) =>
        link.id.length > 0 &&
        link.id.length <= TEXT_LIMIT.id &&
        ids.has(link.source) &&
        ids.has(link.target) &&
        link.source !== link.target,
    )
  );
}

/** Absolute canvas position of a node, resolving one level of area parenting. */
export function absolutePosition(node: WorkNode, nodes: readonly WorkNode[]): CanvasPosition {
  if (!node.parentId) return { ...node.position };
  const parent = nodes.find((candidate) => candidate.id === node.parentId);
  return parent
    ? { x: node.position.x + parent.position.x, y: node.position.y + parent.position.y }
    : { ...node.position };
}

/** Position once. Subsequent projection changes preserve user arrangement and node identity. */
export function reconcileNodes(
  previous: WorkNode[],
  items: readonly CanvasItem[],
  positions: Readonly<Record<string, CanvasPosition>> = {},
  sizes: Readonly<Record<string, CanvasSize>> = {},
  areas: readonly CanvasArea[] = [],
  areaPlacements: Readonly<Record<string, CanvasPosition & CanvasSize>> = {},
): WorkNode[] {
  const existing = new Map(previous.map((node) => [node.id, node]));
  const areaNodes: Node<AreaData, "area">[] = areas.map((area, index) => {
    const id = areaNodeId(area.id);
    const count = items.filter((item) => item.area === area.id).length;
    const node = existing.get(id);
    if (node && isAreaNode(node)) {
      return node.data.title === area.title && node.data.count === count
        ? node
        : { ...node, data: { title: area.title, count } };
    }
    const placement = areaPlacements[area.id];
    const valid = placement && validPosition(placement) && validAreaSize(placement);
    return {
      id,
      type: "area",
      position: valid
        ? { x: placement.x, y: placement.y }
        : { x: 80 + index * 60, y: 80 + index * 60 },
      width: valid ? placement.width : DEFAULT_AREA.width,
      height: valid ? placement.height : DEFAULT_AREA.height,
      data: { title: area.title, count },
      dragHandle: ".area-title",
      deletable: false,
      connectable: false,
      selectable: true,
      zIndex: -1,
      ariaLabel: area.title,
    };
  });
  const areaById = new Map(areaNodes.map((node) => [node.id, node]));
  const occupied = items.flatMap((item) => {
    const node = existing.get(item.id);
    return node ? [absolutePosition(node, previous)] : [];
  });
  function nextPosition(index: number): CanvasPosition {
    let position: CanvasPosition;
    do {
      position = { x: 80 + (index % 3) * 520, y: 120 + Math.floor(index / 3) * 420 };
      index++;
    } while (
      occupied.some(
        (other) => Math.abs(other.x - position.x) < 500 && Math.abs(other.y - position.y) < 400,
      )
    );
    return position;
  }
  const next: WorkNode[] = items.map((item, index): WorkNode => {
    const node = existing.get(item.id);
    const parentId = item.area ? areaNodeId(item.area) : undefined;
    const parent = parentId ? areaById.get(parentId) : undefined;
    if (node && isItemNode(node)) {
      const reparented = (node.parentId ?? undefined) !== (parent ? parentId : undefined);
      const same =
        !reparented &&
        node.data.title === item.title &&
        node.data.kind === item.kind &&
        node.data.type === item.type &&
        node.data.area === item.area &&
        node.data.unavailable === item.unavailable &&
        node.data.detail === item.detail &&
        node.data.status === item.status &&
        node.data.favicon === item.favicon &&
        node.data.artifact === item.artifact &&
        node.data.layout === item.layout &&
        node.data.actionLabel === item.actionLabel &&
        node.data.subject === item.subject &&
        node.data.finding === item.finding &&
        node.data.decision === item.decision &&
        node.data.active === item.active &&
        JSON.stringify(node.data.sources) === JSON.stringify(item.sources) &&
        node.data.media?.asset.digest === item.media?.asset.digest &&
        node.data.image?.digest === item.image?.digest &&
        JSON.stringify(node.data.agent) === JSON.stringify(item.agent) &&
        JSON.stringify(node.data.page) === JSON.stringify(item.page) &&
        JSON.stringify(node.data.facts) === JSON.stringify(item.facts) &&
        JSON.stringify(node.data.responsibility) === JSON.stringify(item.responsibility) &&
        JSON.stringify(node.data.findings) === JSON.stringify(item.findings) &&
        JSON.stringify(node.data.file) === JSON.stringify(item.file) &&
        JSON.stringify(node.data.command) === JSON.stringify(item.command);
      // Agents follow their work: a fresh computed position moves the node.
      const moved = item.agent ? positions[item.id] : undefined;
      const relocated =
        !!moved &&
        validPosition(moved) &&
        !node.dragging &&
        (node.position.x !== moved.x || node.position.y !== moved.y);
      if (same && !relocated) return node;
      if (!reparented)
        return {
          ...node,
          data: item,
          ariaLabel: `${item.title}. ${item.status}`,
          ...(relocated ? { position: { ...moved } } : {}),
        };
      const absolute = absolutePosition(node, previous);
      return {
        ...node,
        data: item,
        ariaLabel: `${item.title}. ${item.status}`,
        parentId: parent ? parentId : undefined,
        position: parent
          ? { x: absolute.x - parent.position.x, y: absolute.y - parent.position.y }
          : absolute,
      };
    }
    const restored = Object.hasOwn(positions, item.id) ? positions[item.id] : undefined;
    const absolute = validPosition(restored) ? { ...restored! } : nextPosition(index);
    occupied.push(absolute);
    const position = parent
      ? { x: absolute.x - parent.position.x, y: absolute.y - parent.position.y }
      : absolute;
    const size = sizes[item.id];
    const restoredSize =
      size &&
      Number.isInteger(size.width) &&
      Number.isInteger(size.height) &&
      size.width >= 120 &&
      size.width <= 4096 &&
      size.height >= 80 &&
      size.height <= 4096
        ? size
        : undefined;
    return {
      id: item.id,
      type: "work",
      position,
      ...(parent ? { parentId } : {}),
      data: item,
      width: restoredSize?.width ?? defaultSize(item).width,
      height: restoredSize?.height ?? defaultSize(item).height,
      dragHandle: ".work-drag-handle",
      deletable: false,
      connectable: false,
      class: item.agent ? "agent-node work-node-enter" : "work-node-enter",
      ariaLabel: `${item.title}. ${item.status}`,
    };
  });
  // Clusters are derived from the cards afterwards; they keep their node until then.
  const clusters = previous.filter(isClusterNode);
  const combined: WorkNode[] = [...areaNodes, ...clusters, ...next];
  return combined.length === previous.length &&
    combined.every((node, index) => node === previous[index])
    ? previous
    : combined;
}

/**
 * Cluster nodes around their members as they stand now, 12 px out. Members are
 * fixed by projection, so a card dragged away stretches its cluster.
 */
export function withClusters(
  nodes: WorkNode[],
  clusters: readonly CanvasCluster[],
  active: ReadonlySet<string> = new Set(),
): WorkNode[] {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const derived: ClusterNode[] = [];
  for (const cluster of clusters) {
    const members = cluster.members.flatMap((id) => {
      const node = byId.get(id);
      return node && isItemNode(node) ? [node] : [];
    });
    if (!members.length) continue;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const node of members) {
      const p = absolutePosition(node, nodes);
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x + (node.measured?.width ?? node.width ?? 280));
      maxY = Math.max(maxY, p.y + (node.measured?.height ?? node.height ?? 160));
    }
    const position = { x: Math.round(minX - CLUSTER_PAD), y: Math.round(minY - CLUSTER_PAD) };
    const width = Math.round(maxX - minX + CLUSTER_PAD * 2);
    const height = Math.round(maxY - minY + CLUSTER_PAD * 2);
    const data = {
      label: cluster.label,
      more: cluster.more,
      active: cluster.members.some((id) => active.has(id)),
    };
    const node = byId.get(cluster.id);
    derived.push(
      node &&
        isClusterNode(node) &&
        node.position.x === position.x &&
        node.position.y === position.y &&
        node.width === width &&
        node.height === height &&
        node.data.label === data.label &&
        node.data.more === data.more &&
        node.data.active === data.active
        ? node
        : {
            id: cluster.id,
            type: "cluster",
            position,
            width,
            height,
            data,
            class: "cluster-node",
            selectable: false,
            draggable: false,
            focusable: false,
            deletable: false,
            connectable: false,
            zIndex: -1,
            ariaLabel: cluster.label,
          },
    );
  }
  const others = nodes.filter((node) => !isClusterNode(node));
  const areas = others.filter(isAreaNode);
  const cards = others.filter((node) => !isAreaNode(node));
  const next = [...areas, ...derived, ...cards];
  return next.length === nodes.length && next.every((node, index) => node === nodes[index])
    ? nodes
    : next;
}

/** Applies a remote view to existing nodes in place; dragging and derived nodes keep local geometry. */
export function applyRemoteView(
  previous: WorkNode[],
  view: CanvasView,
  authoritative: ReadonlySet<string>,
): WorkNode[] {
  let changed = false;
  const areaMoves = new Map<string, CanvasPosition>();
  const next = previous.map((node): WorkNode => {
    if (node.dragging) return node;
    if (isAreaNode(node)) {
      const placement = view.areas?.[node.id.slice(AREA_PREFIX.length)];
      if (!placement || !validPosition(placement) || !validAreaSize(placement)) return node;
      if (
        node.position.x === placement.x &&
        node.position.y === placement.y &&
        node.width === placement.width &&
        node.height === placement.height
      )
        return node;
      changed = true;
      areaMoves.set(node.id, { x: placement.x, y: placement.y });
      return {
        ...node,
        position: { x: placement.x, y: placement.y },
        width: placement.width,
        height: placement.height,
      };
    }
    if (!authoritative.has(node.id)) return node;
    const position = view.positions[node.id];
    const size = view.sizes?.[node.id];
    const parent = node.parentId
      ? (areaMoves.get(node.parentId) ??
        previous.find((candidate) => candidate.id === node.parentId)?.position)
      : undefined;
    const relative = validPosition(position)
      ? parent
        ? { x: position.x - parent.x, y: position.y - parent.y }
        : { ...position }
      : undefined;
    const samePosition =
      !relative || (node.position.x === relative.x && node.position.y === relative.y);
    const sameSize = !validSize(size) || (node.width === size.width && node.height === size.height);
    if (samePosition && sameSize) return node;
    changed = true;
    return {
      ...node,
      ...(samePosition ? {} : { position: relative! }),
      ...(sameSize ? {} : { width: size!.width, height: size!.height }),
    };
  });
  return changed ? next : previous;
}

/** Area containing the node's centre, if any. */
export function containingArea(node: WorkNode, nodes: readonly WorkNode[]): string | null {
  if (isAreaNode(node)) return null;
  const absolute = absolutePosition(node, nodes);
  const cx = absolute.x + (node.width ?? 280) / 2;
  const cy = absolute.y + (node.height ?? 160) / 2;
  for (const candidate of nodes) {
    if (!isAreaNode(candidate)) continue;
    const w = candidate.width ?? DEFAULT_AREA.width;
    const h = candidate.height ?? DEFAULT_AREA.height;
    if (
      cx >= candidate.position.x &&
      cx <= candidate.position.x + w &&
      cy >= candidate.position.y &&
      cy <= candidate.position.y + h
    )
      return candidate.id.slice(AREA_PREFIX.length);
  }
  return null;
}

/** Bounds around the given nodes in absolute canvas coordinates. */
export function nodesBounds(
  ids: readonly string[],
  nodes: readonly WorkNode[],
  padding = 32,
): (CanvasPosition & CanvasSize) | null {
  const selected = nodes.filter((node) => ids.includes(node.id) && isItemNode(node));
  if (!selected.length) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const node of selected) {
    const p = absolutePosition(node, nodes);
    minX = Math.min(minX, p.x);
    minY = Math.min(minY, p.y);
    maxX = Math.max(maxX, p.x + (node.width ?? 280));
    maxY = Math.max(maxY, p.y + (node.height ?? 160));
  }
  return {
    x: Math.round(minX - padding),
    y: Math.round(minY - padding - 36),
    width: Math.max(240, Math.round(maxX - minX + padding * 2)),
    height: Math.max(160, Math.round(maxY - minY + padding * 2 + 36)),
  };
}

/** Invalid saved geometry never reaches XYFlow. User arrangement is view state only. */
export function validViewport(
  value: CanvasView["viewport"] | undefined,
): CanvasView["viewport"] | undefined {
  return value &&
    Number.isFinite(value.x) &&
    Number.isFinite(value.y) &&
    Math.abs(value.x) <= 1_000_000 &&
    Math.abs(value.y) <= 1_000_000 &&
    Number.isFinite(value.zoom) &&
    value.zoom >= 0.2 &&
    value.zoom <= 2
    ? { ...value }
    : undefined;
}

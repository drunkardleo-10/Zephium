import type { MediaAssetV1 } from "$domain/resources";
import type { ArtifactView, FindingView, SubjectView } from "$shared/ui/data/Artifact";
import type { Node } from "@xyflow/svelte";

type CanvasKind =
  | "tab"
  | "note"
  | "media"
  | "objective"
  | "responsibility"
  | "result"
  | "subject"
  | "finding"
  | "source"
  | "page"
  | "agent";
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
  /** Transient agent presence: avatar seed, status, and its latest line. */
  agent?: { seed: number; activity: string; objective: string; line?: string; worker?: boolean };
  /** A cited public source; opening it goes through the pane. */
  source?: { url: string; role: string };
  /** A page a browser step opened: its newest frame while the agent works there. */
  page?: { url: string; host: string; frame: string | null; live: boolean };
  unavailable?: boolean;
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
  /** `working` is the transient tie between an agent and what it acts on now. */
  kind: "dependency" | "reference" | "working" | RelationKind;
  label?: string;
};
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
export type WorkItemNode = Node<CanvasItem, "work">;
export type WorkNode = WorkItemNode | Node<AreaData, "area">;
const AREA_PREFIX = "area:";
const areaNodeId = (id: string) => `${AREA_PREFIX}${id}`;
export const isAreaNode = (node: WorkNode): node is Node<AreaData, "area"> => node.type === "area";
const DEFAULT_AREA: CanvasSize = { width: 640, height: 420 };
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
  if (item.artifact) {
    switch (item.artifact.content.kind) {
      case "comparison":
        return { width: 640, height: 360 };
      case "table":
        return { width: 560, height: 320 };
      case "chart":
        return { width: 480, height: 320 };
      case "checklist":
        return { width: 360, height: 300 };
      case "sources":
        return { width: 320, height: 240 };
      case "browser":
        return { width: 320, height: 180 };
      default:
        return { width: 480, height: 360 };
    }
  }
  switch (item.type) {
    case "tab":
      return { width: 280, height: 96 };
    case "subject":
      return { width: 240, height: 112 };
    case "finding":
      return { width: 300, height: 140 };
    case "source":
      return { width: 260, height: 84 };
    case "note":
      return { width: 300, height: 200 };
    case "media":
      return { width: 280, height: 230 };
    case "objective":
      return { width: 320, height: 150 };
    case "responsibility":
      return { width: 280, height: 150 };
    case "page":
      return { width: 320, height: 236 };
    case "agent":
      return item.agent?.worker
        ? { width: 200, height: 64 }
        : { width: 260, height: item.agent?.line ? 104 : 84 };
    default:
      return { width: 280, height: 160 };
  }
}
const CANVAS_LINK_LIMIT = 2000;

export function validScene(items: readonly CanvasItem[], links: readonly CanvasLink[]): boolean {
  if (items.length > CANVAS_ITEM_LIMIT || links.length > CANVAS_LINK_LIMIT) return false;
  const ids = new Set(items.map((item) => item.id));
  return (
    ids.size === items.length &&
    items.every(
      (item) =>
        item.id.length > 0 &&
        item.id.length <= 128 &&
        item.title.length <= 512 &&
        item.detail.length <= 2048 &&
        item.kind.length <= 128 &&
        item.status.length <= 256,
    ) &&
    new Set(links.map((link) => link.id)).size === links.length &&
    links.every(
      (link) =>
        link.id.length > 0 &&
        link.id.length <= 128 &&
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
    if (node && !isAreaNode(node)) {
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
        JSON.stringify(node.data.source) === JSON.stringify(item.source) &&
        node.data.media?.asset.digest === item.media?.asset.digest &&
        node.data.image?.digest === item.image?.digest &&
        JSON.stringify(node.data.agent) === JSON.stringify(item.agent) &&
        JSON.stringify(node.data.responsibility) === JSON.stringify(item.responsibility);
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
      ...(item.agent ? { class: "agent-node" } : {}),
      ariaLabel: `${item.title}. ${item.status}`,
    };
  });
  const combined: WorkNode[] = [...areaNodes, ...next];
  return combined.length === previous.length &&
    combined.every((node, index) => node === previous[index])
    ? previous
    : combined;
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
  const selected = nodes.filter((node) => ids.includes(node.id) && !isAreaNode(node));
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

import type { ArtifactView } from "$shared/ui/data/Artifact";
import type { Node } from "@xyflow/svelte";

/** Display values only; deliberately independent from the generated Work wire contract. */
export type CanvasItem = {
  id: string;
  title: string;
  kind: string;
  detail: string;
  status: string;
  /** Fixed-raster native favicon only; never a remote image URL. */
  favicon?: string | null;
  artifact?: ArtifactView;
  layout?: "artifact";
  actionLabel?: string;
  /** Planned output names only; these are not produced artifact resources. */
  responsibility?: { outputs: string[] };
};
export type CanvasLink = {
  id: string;
  source: string;
  target: string;
  kind: "dependency" | "reference";
};
export type CanvasPosition = { x: number; y: number };
export type CanvasView = {
  positions: Record<string, CanvasPosition>;
  sizes?: Record<string, { width: number; height: number }>;
  viewport: { x: number; y: number; zoom: number };
};
export type WorkNode = Node<CanvasItem, "work">;
const CANVAS_ITEM_LIMIT = 500;
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

/** Position once. Subsequent projection changes preserve user arrangement and node identity. */
export function reconcileNodes(
  previous: WorkNode[],
  items: readonly CanvasItem[],
  positions: Readonly<Record<string, CanvasPosition>> = {},
  sizes: Readonly<Record<string, { width: number; height: number }>> = {},
): WorkNode[] {
  const existing = new Map(previous.map((node) => [node.id, node]));
  const occupied = items.flatMap((item) => {
    const node = existing.get(item.id);
    return node ? [node.position] : [];
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
  const next = items.map((item, index): WorkNode => {
    const node = existing.get(item.id);
    if (node) {
      const same =
        node.data.title === item.title &&
        node.data.kind === item.kind &&
        node.data.detail === item.detail &&
        node.data.status === item.status &&
        node.data.favicon === item.favicon &&
        node.data.artifact === item.artifact &&
        node.data.layout === item.layout &&
        node.data.actionLabel === item.actionLabel &&
        JSON.stringify(node.data.responsibility) === JSON.stringify(item.responsibility);
      return same ? node : { ...node, data: item, ariaLabel: `${item.title}. ${item.status}` };
    }
    const restored = Object.hasOwn(positions, item.id) ? positions[item.id] : undefined;
    const position =
      restored &&
      Number.isFinite(restored.x) &&
      Number.isFinite(restored.y) &&
      Math.abs(restored.x) <= 1_000_000 &&
      Math.abs(restored.y) <= 1_000_000
        ? { ...restored }
        : nextPosition(index);
    occupied.push(position);
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
      data: item,
      width: restoredSize?.width ?? (item.layout === "artifact" ? 480 : 280),
      height: restoredSize?.height ?? (item.layout === "artifact" ? 360 : 160),
      dragHandle: ".work-drag-handle",
      deletable: false,
      connectable: false,
      ariaLabel: `${item.title}. ${item.status}`,
    };
  });
  return next.length === previous.length && next.every((node, index) => node === previous[index])
    ? previous
    : next;
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

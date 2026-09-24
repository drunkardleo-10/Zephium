import { bounds, grid, type Placement } from "./arrange";
import type { CanvasPosition, CanvasSize } from "./canvas-model";

/** Card sizes the run places and the cards are built to. */
export const SIZES = {
  request: { width: 300, height: 110 },
  sources: { width: 300, height: 200 },
  page: { width: 248, height: 168 },
  subject: { width: 220, height: 136 },
  pictured: { width: 220, height: 248 },
  findings: { width: 300, height: 200 },
  document: { width: 420, height: 300 },
  comparison: { width: 520, height: 320 },
  result: { width: 420, height: 300 },
  file: { width: 248, height: 96 },
  command: { width: 248, height: 120 },
} as const;
const CARD_GAP = 20;
const CLUSTER_GAP = 48;
export const STAGE_GAP = 56;
/** The air between the agent and the cluster it works beside. */
const STAND_GAP = 16;

/** A stage reads left to right in this order; empty clusters take no width. */
const CLUSTER_ORDER = ["sources", "pages", "work", "subjects", "findings", "results"] as const;
export type ClusterKind = (typeof CLUSTER_ORDER)[number];
/** Past these a cluster's cards stop and its label counts the rest. */
export const CLUSTER_CAP = { pages: 8, subjects: 12 } as const;
const GRID: Record<ClusterKind, { columns: number; cap: number }> = {
  sources: { columns: 1, cap: Infinity },
  pages: { columns: 2, cap: CLUSTER_CAP.pages },
  work: { columns: 2, cap: Infinity },
  subjects: { columns: 3, cap: CLUSTER_CAP.subjects },
  findings: { columns: 1, cap: Infinity },
  results: { columns: 1, cap: Infinity },
};

/** One card of a cluster; `placed` is a saved placement, which is never rewritten. */
export type StageMember = { id: string; size: CanvasSize; placed?: CanvasPosition };
/** Members in projection order; `more` counts what the run knows but no card shows. */
export type StageContents = Partial<
  Record<ClusterKind, { members: readonly StageMember[]; more?: number }>
>;
type StageCluster = {
  kind: ClusterKind;
  /** The members that are cards; the rest only count in `more`. */
  members: string[];
  more: number;
  /** Where its cards stand now: saved placements and new slots together. */
  box: Placement;
};
export type StageLayout = {
  clusters: StageCluster[];
  /** Every visible member: its saved placement, or the slot a new card takes. */
  positions: Record<string, CanvasPosition>;
  /** The bottom of the tallest cluster; the next request stands under it. */
  extent: number;
};

/**
 * Clusters left to right from the request, each a row-major grid anchored at
 * the stage's top. A cluster that already has cards keeps its x, and a cluster
 * to its left narrows rather than grow into it.
 */
export function stageLayout(stage: Placement, contents: StageContents): StageLayout {
  const present = CLUSTER_ORDER.filter((kind) => contents[kind]?.members.length);
  const sticky = new Map<ClusterKind, number>();
  for (const kind of present) {
    const first = contents[kind]!.members[0]!.placed;
    if (first) sticky.set(kind, first.x);
  }
  const clusters: StageCluster[] = [];
  const positions: Record<string, CanvasPosition> = {};
  let extent = stage.y + stage.height;
  let x = stage.x + stage.width + CLUSTER_GAP;
  for (const [order, kind] of present.entries()) {
    const { members, more = 0 } = contents[kind]!;
    const { columns, cap } = GRID[kind];
    const visible = members.slice(0, cap);
    const origin = sticky.get(kind) ?? x;
    const width = Math.max(...visible.map((member) => member.size.width));
    const limit = present
      .slice(order + 1)
      .map((next) => sticky.get(next))
      .find((next): next is number => next !== undefined && next > origin);
    const fits =
      limit === undefined
        ? columns
        : Math.max(1, Math.floor((limit - CLUSTER_GAP - origin + CARD_GAP) / (width + CARD_GAP)));
    const slots = grid(
      visible.map((member) => member.size),
      {
        columns: Math.min(columns, visible.length, fits),
        gap: CARD_GAP,
        origin: { x: origin, y: stage.y },
      },
    );
    const rects = visible.map((member, index) =>
      member.placed ? { ...member.size, ...member.placed } : slots[index]!,
    );
    visible.forEach((member, index) => {
      positions[member.id] = { x: rects[index]!.x, y: rects[index]!.y };
    });
    const box = bounds(rects)!;
    extent = Math.max(extent, box.y + box.height);
    clusters.push({
      kind,
      members: visible.map((member) => member.id),
      more: more + members.length - visible.length,
      box,
    });
    const reserved = bounds(slots)!;
    x = Math.max(x, origin) + reserved.width + CLUSTER_GAP;
  }
  return { clusters, positions, extent };
}

/** Where the agent waits: right of the stage's newest cluster, level with its top. */
export function stageStand(layout: StageLayout): CanvasPosition | undefined {
  const box = layout.clusters.at(-1)?.box;
  return box ? { x: box.x + box.width + STAND_GAP, y: box.y } : undefined;
}

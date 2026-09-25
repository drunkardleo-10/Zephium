import { bounds, grid, type Placement } from "./arrange";
import type { CanvasPosition, CanvasSize } from "./canvas-model";

/** Card sizes the run places where a card does not size itself to what it says. */
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

/**
 * The lane geometry: a request column, then canvas-wide slots a gutter apart;
 * groups pad their cards and carry a caption above them; a group's steps sit
 * in a smaller box of their own.
 */
export const LANE = {
  column: 300,
  gutter: 48,
  slot: 320,
  between: 96,
  pad: 24,
  gap: 16,
  caption: 20,
  inset: 16,
} as const;

/** What a group's cards are, by where they come from. */
export type ClusterKind =
  "sources" | "pages" | "work" | "subjects" | "findings" | "results" | "plan" | "diagram";
/** Worked with, Found, Made: a lane fills its slots in this order. */
type GroupKind = "worked" | "found" | "made";
/** The column each group belongs to when the lane has every group before it. */
const NATURAL: Record<GroupKind, number> = { worked: 0, found: 1, made: 2 };
/** Past these a group's cards stop and its caption counts the rest. */
export const CLUSTER_CAP = { pages: 8, subjects: 12 } as const;

/**
 * One card of a lane. A step or a diagram's part names the result it belongs
 * to; a part says where it stands in its diagram; the cover is the document
 * the rest of Made hangs from. A bare result draws no card of its own: its
 * area or its steps stand in its place, and `extent` is its diagram's whole
 * room, plates included, from the first part.
 */
export type StageMember = {
  id: string;
  size: CanvasSize;
  of?: string;
  at?: CanvasPosition;
  cover?: boolean;
  bare?: boolean;
  extent?: CanvasSize;
};
/** Members in projection order; `more` counts what the run knows but no card shows. */
export type StageContents = Partial<
  Record<ClusterKind, { members: readonly StageMember[]; more?: number }>
>;

type Rect = Placement;
/** A group laid out at the origin: its box, then its cards inside it. */
type GroupShape = {
  kind: GroupKind;
  width: number;
  height: number;
  cards: { id: string; rect: Rect }[];
  counts: Partial<Record<ClusterKind, number>>;
  more: number;
  local?: Rect;
  steps?: { result: string; box: Rect; members: string[] }[];
  diagrams?: { result: string; box: Rect; members: string[] }[];
};
/** A lane's groups before the canvas gives them slots. */
export type LaneShape = GroupShape[];

export type LaneGroup = {
  kind: GroupKind;
  box: Placement;
  /** The cards shown, in reading order; hidden ones only count. */
  members: string[];
  counts: Partial<Record<ClusterKind, number>>;
  more: number;
  /** Worked with: the row of folders, files and commands. */
  local?: Placement;
  /** Made: each result's steps, in a box to its right. */
  steps?: { result: string; box: Placement; members: string[] }[];
  /** Made: each diagram's parts, in an area beside its cover. */
  diagrams?: { result: string; box: Placement; members: string[] }[];
};
export type StageLayout = {
  request: Placement;
  groups: LaneGroup[];
  /** Where every shown card stands, derived from the lane alone. */
  positions: Record<string, CanvasPosition>;
  /** The bottom of the lane's tallest box; the next lane starts 96 px under it. */
  extent: number;
};

const shown = (contents: StageContents, kind: ClusterKind) => {
  const members = contents[kind]?.members ?? [];
  const cap = kind === "pages" || kind === "subjects" ? CLUSTER_CAP[kind] : Infinity;
  const visible = members.slice(0, cap);
  return { visible, hidden: (contents[kind]?.more ?? 0) + members.length - visible.length };
};

function cells(members: readonly StageMember[], columns: number, origin: CanvasPosition): Rect[] {
  return members.length
    ? grid(
        members.map((member) => member.size),
        { columns: Math.min(columns, members.length), gap: LANE.gap, origin },
      )
    : [];
}

/** The group's box around its cards: padding on every side, the caption above them. */
function frame(rects: Rect[]): { width: number; height: number } {
  const box = bounds(rects)!;
  return { width: box.x + box.width + LANE.pad, height: box.y + box.height + LANE.pad };
}

/** Worked with: the web row (Sources, then pages two across) above the local row. */
function worked(contents: StageContents): GroupShape | null {
  const sources = shown(contents, "sources");
  const pages = shown(contents, "pages");
  const local = shown(contents, "work");
  if (!sources.visible.length && !pages.visible.length && !local.visible.length) return null;
  const top = LANE.pad + LANE.caption;
  const column = cells(sources.visible, 1, { x: LANE.pad, y: top });
  const left = column.length ? bounds(column)!.x + bounds(column)!.width + LANE.gap : LANE.pad;
  const read = cells(pages.visible, 2, { x: left, y: top });
  const web = bounds([...column, ...read]);
  const row = cells(local.visible, 2, {
    x: LANE.pad,
    y: web ? web.y + web.height + LANE.gap : top,
  });
  const rects = [...column, ...read, ...row];
  const ids = [...sources.visible, ...pages.visible, ...local.visible].map((member) => member.id);
  return {
    kind: "worked",
    ...frame(rects),
    cards: ids.map((id, index) => ({ id, rect: rects[index]! })),
    counts: {
      sources: contents.sources?.members.length ?? 0,
      pages: (contents.pages?.members.length ?? 0) + (contents.pages?.more ?? 0),
      work: contents.work?.members.length ?? 0,
    },
    more: sources.hidden + pages.hidden + local.hidden,
    ...(row.length ? { local: bounds(row)! } : {}),
  };
}

/** Found: subjects three across, the findings under them. */
function found(contents: StageContents): GroupShape | null {
  const subjects = shown(contents, "subjects");
  const findings = shown(contents, "findings");
  if (!subjects.visible.length && !findings.visible.length) return null;
  const top = LANE.pad + LANE.caption;
  const named = cells(subjects.visible, 3, { x: LANE.pad, y: top });
  const above = bounds(named);
  const claims = cells(findings.visible, 2, {
    x: LANE.pad,
    y: above ? above.y + above.height + LANE.gap : top,
  });
  const rects = [...named, ...claims];
  const ids = [...subjects.visible, ...findings.visible].map((member) => member.id);
  return {
    kind: "found",
    ...frame(rects),
    cards: ids.map((id, index) => ({ id, rect: rects[index]! })),
    counts: {
      subjects: subjects.visible.length + subjects.hidden,
      findings: findings.visible.length,
    },
    more: subjects.hidden + findings.hidden,
  };
}

/** Steps read as a landscape grid: two across up to four, three up to nine, then four. */
export const stepColumns = (count: number) => (count <= 4 ? 2 : count <= 9 ? 3 : 4);

/**
 * Made as a set: the cover, each diagram with its area, and any result with
 * steps stand on rows of their own, the steps or the area to the result's
 * right, or in its place when the result is bare; tables, charts, findings
 * and comparisons follow two across.
 */
function made(contents: StageContents): GroupShape | null {
  const results = shown(contents, "results").visible;
  if (!results.length) return null;
  const plan = contents.plan?.members ?? [];
  const parts = contents.diagram?.members ?? [];
  const alone = (result: StageMember) =>
    !!result.cover ||
    plan.some((step) => step.of === result.id) ||
    parts.some((part) => part.of === result.id);
  const rows = results.filter(alone);
  const rest = results.filter((result) => !alone(result));
  const column = Math.max(
    0,
    ...rows.filter((result) => !result.bare).map((result) => result.size.width),
  );
  const cards: GroupShape["cards"] = [];
  const steps: NonNullable<GroupShape["steps"]> = [];
  const diagrams: NonNullable<GroupShape["diagrams"]> = [];
  let y = LANE.pad + LANE.caption;
  for (const result of rows) {
    let bottom = y;
    if (!result.bare) {
      const rect = { x: LANE.pad, y, ...result.size };
      cards.push({ id: result.id, rect });
      bottom = rect.y + rect.height;
    }
    const own = plan.filter((step) => step.of === result.id);
    if (own.length) {
      const x = result.bare ? LANE.pad : LANE.pad + column + LANE.gutter;
      const placed = cells(own, stepColumns(own.length), {
        x: x + LANE.inset,
        y: y + LANE.inset + LANE.caption,
      });
      const inner = bounds(placed)!;
      const box = {
        x,
        y,
        width: inner.width + LANE.inset * 2,
        height: inner.height + LANE.inset * 2 + LANE.caption,
      };
      own.forEach((step, index) => cards.push({ id: step.id, rect: placed[index]! }));
      steps.push({ result: result.id, box, members: own.map((step) => step.id) });
      bottom = Math.max(bottom, box.y + box.height);
    }
    const drawn = parts.filter((part) => part.of === result.id);
    if (drawn.length) {
      const x = result.bare ? LANE.pad : LANE.pad + result.size.width + LANE.gap;
      const left = x + LANE.inset;
      const top = y + LANE.inset + LANE.caption;
      const placed = drawn.map((part) => ({
        x: left + (part.at?.x ?? 0),
        y: top + (part.at?.y ?? 0),
        ...part.size,
      }));
      const inner = bounds(placed)!;
      // The area holds its parts and every plate between and under them.
      const box = {
        x,
        y,
        width: Math.max(inner.x + inner.width, left + (result.extent?.width ?? 0)) - x + LANE.inset,
        height:
          Math.max(inner.y + inner.height, top + (result.extent?.height ?? 0)) - y + LANE.inset,
      };
      drawn.forEach((part, index) => cards.push({ id: part.id, rect: placed[index]! }));
      diagrams.push({ result: result.id, box, members: drawn.map((part) => part.id) });
      bottom = Math.max(bottom, box.y + box.height);
    }
    y = bottom + LANE.gap;
  }
  cells(rest, 2, { x: LANE.pad, y }).forEach((rect, index) =>
    cards.push({ id: rest[index]!.id, rect }),
  );
  const boxes = [
    ...cards.map((card) => card.rect),
    ...steps.map((entry) => entry.box),
    ...diagrams.map((entry) => entry.box),
  ];
  return {
    kind: "made",
    ...frame(boxes),
    cards,
    counts: { results: results.length, plan: steps.reduce((n, s) => n + s.members.length, 0) },
    more: 0,
    ...(steps.length ? { steps } : {}),
    ...(diagrams.length ? { diagrams } : {}),
  };
}

/** Past this height a group's lines meet its first row, not its middle. */
const ANCHOR_FROM = 240;
/**
 * Where a group's lines attach, from its top: the middle of its first row,
 * caption included, once the group is tall; otherwise nothing, its centre.
 */
export function firstRowAnchor(
  box: { y: number; height: number },
  rows: readonly { y: number; height: number }[],
  inset: number,
): number | undefined {
  if (box.height < ANCHOR_FROM || !rows.length) return undefined;
  const first = Math.min(...rows.map((rect) => rect.y));
  const bottom = Math.max(
    ...rows.filter((rect) => rect.y - first < LANE.gap).map((rect) => rect.y + rect.height),
  );
  return Math.round((inset + bottom - box.y) / 2);
}

/** A lane's groups in reading order, each measured at the origin; empty ones take no slot. */
export function laneShape(contents: StageContents): LaneShape {
  return [worked(contents), found(contents), made(contents)].filter(
    (shape): shape is GroupShape => !!shape,
  );
}

/**
 * Where each slot starts, canvas-wide: a slot is as wide as the widest group
 * any lane puts in it, never under 320, and the request column comes first.
 */
export function laneSlots(shapes: readonly LaneShape[]): number[] {
  const widths: number[] = [];
  for (const shape of shapes)
    shape.forEach((group, slot) => {
      // A group widens a slot only from its own column; a lane missing earlier
      // groups compacts left and runs right on its own instead.
      const width = NATURAL[group.kind] === slot ? group.width : LANE.slot;
      widths[slot] = Math.max(widths[slot] ?? LANE.slot, width);
    });
  const starts: number[] = [];
  let x = LANE.column + LANE.gutter;
  for (const width of widths) {
    starts.push(x);
    x += width + LANE.gutter;
  }
  return starts;
}

const at = (rect: Rect, x: number, y: number): Placement => ({
  x: rect.x + x,
  y: rect.y + y,
  width: rect.width,
  height: rect.height,
});

/** A lane placed on the canvas: its request, then its groups in the slots it fills. */
export function placeLane(request: Placement, shape: LaneShape, slots: readonly number[]) {
  const positions: Record<string, CanvasPosition> = {};
  let extent = request.y + request.height;
  // A lane with more groups than the canvas has slots yet keeps going right.
  let after = request.x + LANE.column + LANE.gutter;
  const groups = shape.map((group, slot): LaneGroup => {
    const x = Math.max(slots[slot] ?? after, after);
    after = x + Math.max(group.width, LANE.slot) + LANE.gutter;
    const y = request.y;
    for (const card of group.cards) positions[card.id] = { x: card.rect.x + x, y: card.rect.y + y };
    extent = Math.max(extent, y + group.height);
    return {
      kind: group.kind,
      box: { x, y, width: group.width, height: group.height },
      members: group.cards.map((card) => card.id),
      counts: group.counts,
      more: group.more,
      ...(group.local ? { local: at(group.local, x, y) } : {}),
      ...(group.diagrams
        ? {
            diagrams: group.diagrams.map((entry) => ({
              result: entry.result,
              box: at(entry.box, x, y),
              members: entry.members,
            })),
          }
        : {}),
      ...(group.steps
        ? {
            steps: group.steps.map((entry) => ({
              result: entry.result,
              box: at(entry.box, x, y),
              members: entry.members,
            })),
          }
        : {}),
    };
  });
  return { request, groups, positions, extent } satisfies StageLayout;
}

/** One lane on its own, in the slots it would take alone. */
export function stageLayout(
  request: Placement,
  contents: StageContents,
  slots?: readonly number[],
): StageLayout {
  const shape = laneShape(contents);
  return placeLane(request, shape, slots ?? laneSlots([shape]));
}

/** The next lane's top: 96 px under the tallest box of the one above. */
export const nextLane = (layout: StageLayout) => layout.extent + LANE.between;

/** The orb is 24 px; a stand is its top-left corner. */
const MARK = 24;
const NEAR = 16;
const OUT = 8;
/** The orb centred on a box's top-left corner. */
const corner = (box: Placement): CanvasPosition => ({ x: box.x - MARK / 2, y: box.y - MARK / 2 });
/** The orb just outside a card's top-right corner. */
const outside = (card: Placement): CanvasPosition => ({
  x: card.x + card.width + OUT,
  y: card.y - OUT - MARK,
});

export type MarkStand =
  | { doing: "thinking" | "searching" | "working" | "writing" }
  | { doing: "reading"; page?: string }
  | { doing: "done"; result?: string };

/**
 * Where the agent's mark stands: beside the request while it thinks, at the
 * corner of Worked with while it searches, just off the page it reads, at the
 * local row while it works there, at the group it writes into, and at the
 * result once it is done. A group not there yet is stood in for by its slot.
 */
export function markStand(
  layout: StageLayout,
  stand: MarkStand,
  sizes: Readonly<Record<string, CanvasSize>> = {},
  slots: readonly number[] = [],
): CanvasPosition {
  const { request } = layout;
  const group = (kind: GroupKind) => layout.groups.find((entry) => entry.kind === kind);
  const card = (id: string | undefined) => {
    const position = id ? layout.positions[id] : undefined;
    const size = id ? sizes[id] : undefined;
    return position && size ? { ...position, ...size } : undefined;
  };
  const beside = { x: request.x + request.width + NEAR, y: request.y };
  const next = () => {
    const last = layout.groups.at(-1)?.box;
    const x =
      slots[layout.groups.length] ??
      (last ? last.x + Math.max(last.width, LANE.slot) : request.x + LANE.column) + LANE.gutter;
    return corner({ x, y: request.y, width: 0, height: 0 });
  };
  const worked = group("worked");
  switch (stand.doing) {
    case "thinking":
      return beside;
    case "searching":
      return worked ? corner(worked.box) : next();
    case "reading": {
      const page = card(stand.page);
      return page ? outside(page) : worked ? corner(worked.box) : next();
    }
    case "working": {
      const row = worked?.local;
      return row ? { x: row.x + row.width + OUT, y: row.y } : worked ? corner(worked.box) : next();
    }
    case "writing": {
      const target = group("made") ?? group("found");
      return target ? corner(target.box) : next();
    }
    case "done": {
      const result = card(stand.result);
      const target = group("made") ?? group("found") ?? worked;
      return result ? outside(result) : target ? corner(target.box) : beside;
    }
  }
}

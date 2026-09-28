import type {
  WorkEnvironmentElement,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import type { MediaAssetV1 } from "$domain/resources";
import { isLive } from "./agent-steps";
import { sourcesSize } from "./card-size";
import { clipText, UNREAD_FOOTER, type CanvasPosition } from "./canvas-model";
import { pageGroups, sourceRows, unreadPages, type PageGroup } from "./project-environment-stage";
import { threadOf, type Branch, type WorkStage } from "./project-environment-thread";
import { boardOf } from "./board/adapter";
import { boardLayout, type LayoutBlock } from "./board/layout";
import { BAND, LANE, placeBand } from "./board/lane";
import { estimate, widthRange } from "./board/size";
import { runTrail } from "./board/trail";
import { plain } from "./board/text";
import type { Block, Picture } from "./board/types";
import * as m from "$shared/i18n/messages";

/** A placement written in board terms: the person moved the block; `x, y` are from the lane's corner. */
export const BOARD_PIN = 3;
/** A branch: its site's line over one row of page frames with their titles, a tile per page up to three. */
const TILE = (BAND.branch - 2 * BAND.across) / 3;
const BRANCH = { height: 176, shown: 3 } as const;
const branchWidth = (pages: number) => {
  const tiles = Math.max(1, Math.min(BRANCH.shown, pages));
  return Math.round(tiles * TILE + (tiles - 1) * BAND.across);
};
const REQUEST_TEXT = 512;

/**
 * Whether an element is drawn by its request's lane: the request itself, or
 * what one of the canvas's works found or made. The person's own elements keep
 * absolute places.
 */
export function laneElement(
  snapshot: WorkEnvironmentSnapshot,
  element: WorkEnvironmentElement,
): boolean {
  const reference = element.reference;
  if (reference.kind === "objective") return true;
  if (reference.kind !== "subject" && reference.kind !== "finding" && reference.kind !== "artifact")
    return false;
  return snapshot.elements.some(
    (candidate) =>
      candidate.reference.kind === "objective" &&
      candidate.reference.objective === reference.objective,
  );
}

/** Blocks the person moved, by their offset from their lane's corner. */
function boardPins(snapshot: WorkEnvironmentSnapshot): Map<string, CanvasPosition> {
  const pins = new Map<string, CanvasPosition>();
  for (const place of snapshot.view.placements)
    if (place.revision === BOARD_PIN) pins.set(place.element, { x: place.x, y: place.y });
  return pins;
}

/** The admitted picture of each element: a `uses` relation to an image resource. */
export function elementPictures(
  snapshot: WorkEnvironmentSnapshot,
  media: ReadonlyMap<string, MediaAssetV1>,
): Map<string, Picture> {
  const resources = new Map(
    snapshot.elements.flatMap((element) =>
      element.reference.kind === "resource" ? [[element.id, element.reference.resource]] : [],
    ),
  );
  const pictures = new Map<string, Picture>();
  for (const relation of snapshot.relations ?? []) {
    if (relation.kind !== "uses" || pictures.has(relation.from)) continue;
    const resource = resources.get(relation.to);
    const asset = resource ? media.get(resource) : undefined;
    if (asset?.kind === "image")
      pictures.set(relation.from, { profile: snapshot.profile, digest: asset.digest });
  }
  return pictures;
}

/**
 * The person's words as text on the canvas: who and when over them, two lines
 * at rest, every line once opened.
 */
export function requestTextSize(text: string, open: boolean) {
  const perLine = 38;
  const lines = Math.max(1, Math.ceil(text.length / perLine));
  const shown = open ? lines : Math.min(2, lines);
  // Never under 80: a checkpoint refuses a shorter placement.
  return { width: BAND.request, height: Math.max(80, 26 + shown * 25 + (lines > 2 ? 22 : 0)) };
}

const siteOf = (url: string) => {
  try {
    return new URL(url).host.replace(/^www\./u, "");
  } catch {
    return "";
  }
};

/** A request's pages by the site they sit on, in the order the runs first went there. */
function branchesOf(card: string, groups: readonly PageGroup[]): Branch[] {
  const bySite = new Map<string, Branch>();
  for (const group of groups) {
    const site = siteOf(group.url);
    if (!site) continue;
    const branch = bySite.get(site) ?? {
      id: `branch:${card}:${site}`,
      host: site,
      pages: [],
      live: false,
    };
    branch.pages.push(group);
    if (group.steps.some((step) => step.status === "running")) branch.live = true;
    bySite.set(site, branch);
  }
  return [...bySite.values()];
}

/** The board's title on up to two lines, its lead at the reading size under it. */
const HEAD = { width: 680 } as const;
function headHeight(title: string, lead: string, more: number, width: number) {
  const titleLines = title ? Math.min(2, Math.ceil((title.length * 12.5) / width)) : 0;
  const leadLines = lead ? Math.ceil((lead.length * 8) / width) : 0;
  const moreLines = more ? Math.ceil((more * 8) / width) + 1 : 0;
  return titleLines * 28 + (title && lead ? 8 : 0) + leadLines * 24 + moreLines * 25;
}

/** A block's height as it last measured itself at this width, open or not. */
export const measureKey = (id: string, width: number, open: boolean) =>
  `${id}|${Math.round(width)}|${open ? 1 : 0}`;

export type StageOptions = {
  recorded?: (objective: string) => readonly WorkPageV1[];
  pictures?: ReadonlyMap<string, Picture>;
  chosen?: ReadonlySet<string>;
  /** Heights blocks and heads measured, by `measureKey`. */
  measured?: ReadonlyMap<string, number>;
  /** The block opened in place, if one is. */
  open?: string | null;
  /** Requests whose words the person opened to read whole. */
  requests?: ReadonlySet<string>;
};

/**
 * Every request of the canvas as a lane, top to bottom: its process column
 * at x = 0 (the request, the trail, the pages it reads while it runs, the
 * sources), its board to the right, lanes 96 px apart.
 */
export function environmentStages(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  options: StageOptions = {},
): WorkStage[] {
  const pins = boardPins(snapshot);
  const measured = options.measured ?? new Map<string, number>();
  const stages: WorkStage[] = [];
  let top = 0;
  for (const element of snapshot.elements) {
    if (element.reference.kind !== "objective") continue;
    const projection = objectives.get(element.reference.objective);
    if (!projection) continue;
    const recorded = options.recorded?.(projection.work.id) ?? [];
    for (const draft of threadOf(element.id, projection)) {
      const runs = draft.executions.flatMap((id) => {
        const execution = projection.executions.find((entry) => entry.id === id);
        return execution ? [execution] : [];
      });
      const live = runs.some((execution) => isLive(projection, execution));
      const trail = runTrail(runs, live);
      const elements = snapshot.elements.filter((candidate) => {
        const reference = candidate.reference;
        return (
          (reference.kind === "artifact" || reference.kind === "subject") &&
          reference.objective === projection.work.id &&
          draft.executions.includes(reference.execution)
        );
      });
      const board = boardOf({
        id: `board:${draft.card}`,
        executions: runs,
        elements,
        ...(options.pictures ? { pictures: options.pictures } : {}),
        ...(options.chosen ? { chosen: options.chosen } : {}),
        ...(live ? { pending: m.work_board_pending() } : {}),
      });
      const request = clipText(draft.request, REQUEST_TEXT);
      const requestPart = {
        id: draft.card,
        ...requestTextSize(request, !!options.requests?.has(draft.card)),
      };
      const branches = branchesOf(
        draft.card,
        runs.flatMap((run) => pageGroups(run, recorded)),
      );
      const rows = new Set(runs.flatMap((run) => sourceRows(run).map((row) => row.key))).size;
      const unread = runs.reduce((sum, run) => sum + unreadPages(run).length, 0);
      const sourcesId = rows || unread ? `sources:${draft.card}` : undefined;
      const sourcesPart = sourcesId
        ? (() => {
            const size = sourcesSize(rows);
            return {
              id: sourcesId,
              width: size.width,
              height: size.height + (unread ? UNREAD_FOOTER : 0),
            };
          })()
        : undefined;
      const open = options.open ?? undefined;
      const sized = (block: Block, width: number, opened: boolean) =>
        measured.get(measureKey(block.id, width, opened)) ?? estimate(block, width, opened);
      const pinned = new Set(
        board.blocks.flatMap((block) => (pins.has(block.id) ? [block.id] : [])),
      );
      const layout = boardLayout(
        board.blocks.map((block): LayoutBlock => ({
          id: block.id,
          kind: block.kind,
          emphasis: block.emphasis,
          ...(block.group ? { group: block.group } : {}),
          width: widthRange(block),
          height: (width) => sized(block, width, block.id === open),
        })),
        { ...(open ? { open } : {}), pinned },
      );
      const headId = board.title || board.lead || board.more ? `head:${draft.card}` : undefined;
      const headWidth = Math.min(layout.width, HEAD.width);
      const head = headId
        ? {
            id: headId,
            width: headWidth,
            height:
              measured.get(measureKey(headId, headWidth, false)) ??
              headHeight(
                board.title,
                board.lead,
                board.more?.blocks.reduce((sum, node) => sum + plain(node).length, 0) ?? 0,
                headWidth,
              ),
          }
        : undefined;
      const pinSizes = new Map(
        board.blocks.flatMap((block) => {
          const pin = pins.get(block.id);
          if (!pin) return [];
          const width = widthRange(block).ideal;
          return [[block.id, { ...pin, width, height: sized(block, width, false) }] as const];
        }),
      );
      const lane = placeBand(top, {
        request: requestPart,
        branches: branches.map((branch) => ({
          id: branch.id,
          width: branchWidth(branch.pages.length),
          height: BRANCH.height,
        })),
        ...(head ? { head } : {}),
        board: layout,
        ...(sourcesPart ? { sources: sourcesPart } : {}),
        pins: pinSizes,
      });
      const place = lane.rects[draft.card]!;
      stages.push({
        element: element.id,
        objective: projection.work.id,
        card: draft.card,
        request,
        executions: draft.executions,
        live,
        place,
        board,
        lane,
        trail,
        pinned,
        column: {
          branches: branches.map((branch) => branch.id),
          ...(sourcesId ? { sources: sourcesId } : {}),
          ...(headId ? { head: headId } : {}),
        },
        branches,
        targets: Object.fromEntries(
          Object.entries(lane.rects).map(([id, rect]) => [id, { x: rect.x, y: rect.y }]),
        ),
      });
      top = lane.extent + LANE.between;
    }
  }
  return stages;
}

/** The execution a stage's pages come from while it runs. */
export function stageRuns(
  stage: WorkStage,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
): WorkExecutionFact[] {
  const projection = objectives.get(stage.objective);
  return stage.executions.flatMap((id) => {
    const execution = projection?.executions.find((entry) => entry.id === id);
    return execution ? [execution] : [];
  });
}

type Box = { x: number; y: number; width: number; height: number };
const overlaps = (a: Box, b: Box) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

/**
 * The person's own cards never sit under a band: one that a band has grown
 * over stands just past its right edge, at the height it had, until the
 * person moves it. Nothing here is saved.
 */
export function clearOfBands(
  positions: Readonly<Record<string, CanvasPosition>>,
  sizes: Readonly<Record<string, { width: number; height: number }>>,
  stages: readonly WorkStage[],
): Record<string, CanvasPosition> {
  const bands = stages.map((stage): Box => {
    const rects = Object.values(stage.lane.rects);
    const right = Math.max(...rects.map((rect) => rect.x + rect.width));
    const top = Math.min(...rects.map((rect) => rect.y));
    return { x: 0, y: top, width: right, height: stage.lane.extent - top };
  });
  const moved: Record<string, CanvasPosition> = {};
  for (const [id, at] of Object.entries(positions)) {
    const size = sizes[id] ?? { width: 280, height: 160 };
    const hit = bands.find((band) => overlaps(band, { ...at, ...size }));
    moved[id] = hit ? { x: hit.x + hit.width + 48, y: at.y } : at;
  }
  return moved;
}

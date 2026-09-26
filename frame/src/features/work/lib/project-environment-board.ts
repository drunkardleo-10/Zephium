import type {
  WorkEnvironmentElement,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import type { MediaAssetV1 } from "$domain/resources";
import { isLive } from "./agent-steps";
import { requestSize, sourcesSize } from "./card-size";
import { clipText, UNREAD_FOOTER, type CanvasPosition } from "./canvas-model";
import { pageGroups, sourceRows, unreadPages, type PageGroup } from "./project-environment-stage";
import { threadOf, type WorkStage } from "./project-environment-thread";
import { boardOf } from "./board/adapter";
import { boardLayout, type LayoutBlock } from "./board/layout";
import { LANE, placeLane } from "./board/lane";
import { estimate, widthRange } from "./board/size";
import { runTrail, type TrailLine } from "./board/trail";
import { plain } from "./board/text";
import type { Block, Picture } from "./board/types";
import * as m from "$shared/i18n/messages";

/** A placement written in board terms: the person moved the block; `x, y` are from the lane's corner. */
export const BOARD_PIN = 3;
/** A page card in the process column while its run reads. */
const PAGE = { width: LANE.column, height: 196 } as const;
/** The column shows this many pages at once; the sources card keeps every frame after. */
const LIVE_PAGES = 2;
/** Captured frames the sources card shows above its rows. */
export const FRAMES = 4;
const FRAME_STRIP = 54;
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

/** A trail line is 20 px, 16 more for its detail, 6 apart, inside 12 px. */
function trailSize(lines: readonly TrailLine[]) {
  const body = lines.reduce((sum, line) => sum + 20 + (line.detail ? 16 : 0), 0);
  return { width: LANE.column, height: 24 + body + Math.max(0, lines.length - 1) * 6 };
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
};

/** The pages the column shows while a run reads: held ones first, then live, then the latest. */
function livePages(groups: readonly PageGroup[]): PageGroup[] {
  const rank = (group: PageGroup) =>
    group.steps.some((step) => step.status === "running") ? 0 : 1;
  return [...groups]
    .map((group, index) => ({ group, index }))
    .sort((a, b) => rank(a.group) - rank(b.group) || b.index - a.index)
    .slice(0, LIVE_PAGES)
    .map((entry) => entry.group);
}

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
      const column: { id: string; width: number; height: number }[] = [
        { id: draft.card, ...requestSize(request) },
      ];
      const trailId = trail.length ? `trail:${draft.card}` : undefined;
      if (trailId) column.push({ id: trailId, ...trailSize(trail) });
      const pages = live ? livePages(runs.flatMap((run) => pageGroups(run, recorded))) : [];
      for (const page of pages) column.push({ id: page.id, ...PAGE });
      const rows = new Set(runs.flatMap((run) => sourceRows(run).map((row) => row.key))).size;
      const unread = runs.reduce((sum, run) => sum + unreadPages(run).length, 0);
      const framed =
        !live && runs.some((run) => pageGroups(run, recorded).some((group) => !!group.page?.frame));
      const sourcesId = rows || unread ? `sources:${draft.card}` : undefined;
      if (sourcesId) {
        const size = sourcesSize(rows);
        column.push({
          id: sourcesId,
          width: size.width,
          height: size.height + (unread ? UNREAD_FOOTER : 0) + (framed ? FRAME_STRIP : 0),
        });
      }
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
      const lane = placeLane(top, {
        column,
        ...(head ? { head } : {}),
        board: layout,
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
          ...(trailId ? { trail: trailId } : {}),
          pages: pages.map((page) => page.id),
          ...(sourcesId ? { sources: sourcesId } : {}),
          ...(headId ? { head: headId } : {}),
        },
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

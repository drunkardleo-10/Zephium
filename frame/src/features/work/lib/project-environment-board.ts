import type {
  WorkEnvironmentElement,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import type { MediaAssetV1 } from "$domain/resources";
import { isLive } from "./agent-steps";
import { clipText, type CanvasPosition } from "./canvas-model";
import { threadOf, type WorkStage } from "./project-environment-thread";
import { boardOf } from "./board/adapter";
import { BOARD, boardLayout, type LayoutBlock } from "./board/layout";
import { estimate, widthRange } from "./board/size";
import { runTrail } from "./board/trail";
import { plain } from "./board/text";
import type { Block, Picture } from "./board/types";
import { RUN, placeRun } from "./run/layout";
import { runParts, type PartAsk, type RunInputView, type RunPart } from "./run/parts";
import { PART, partSize, type PartShape } from "./run/part-size";
import { foundByPart } from "./run/found";
import * as m from "$shared/i18n/messages";

/** A placement written in board terms: the person moved the block; `x, y` are from the result's corner. */
export const BOARD_PIN = 3;
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
  const perLine = 36;
  const lines = Math.max(1, Math.ceil(text.length / perLine));
  const shown = open ? lines : Math.min(2, lines);
  // Never under 80: a checkpoint refuses a shorter placement.
  return { width: RUN.request, height: Math.max(80, 20 + shown * 24 + (lines > 2 ? 24 : 0)) };
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
  /** Questions waiting on the person, by the work they belong to. */
  asks?: (objective: string) => readonly PartAsk[];
  /** What a run drew on before it began. */
  inputs?: (runs: readonly WorkExecutionFact[]) => readonly RunInputView[];
};

/** An input's mark: its glyph and its words on one line. */
const INPUT = { width: 200, height: 28 } as const;

/** How a part's own node stands: frames while it works, a stack once done. */
export function partShape(part: RunPart): PartShape {
  if (part.ask) return { kind: "ask" };
  if (part.helper === "computer" || part.helper === "connection")
    return { kind: "helper", lines: part.lines?.length ?? 0 };
  if (part.helper === "research")
    return {
      kind: "sources",
      rows: part.sources.length,
      more: part.sources.length > PART.sourceRows,
    };
  if (!part.pages.length) return { kind: "label" };
  return part.state === "running" || part.state === "waiting"
    ? { kind: "frames", count: part.pages.length }
    : { kind: "stack", count: part.pages.length };
}

/**
 * Every request of the canvas as a run, top to bottom, 120 apart: its words,
 * a row per part with what the part found at the row's end, the result on
 * the right with the answer at its head.
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
      const alive = (execution: WorkExecutionFact) => isLive(projection, execution);
      const live = runs.some(alive);
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
      const parts = runParts(draft.card, runs, recorded, alive, {
        search: m.work_part_search(),
        unread: m.work_part_failed(),
        computer: m.work_part_computer(),
      });
      const asked = options.asks?.(projection.work.id) ?? [];
      if (live)
        for (const part of parts) {
          const ask = asked.find((entry) => [part.id, part.key, part.host].includes(entry.part));
          if (ask) part.ask = { view: ask.view, props: ask.props };
        }
      const inputs = (options.inputs?.(runs) ?? []).map((input, index) => ({
        id: `input:${draft.card}:${index}`,
        input,
      }));
      const found = foundByPart(board, parts);
      const open = options.open ?? undefined;
      const sized = (block: Block, width: number, opened: boolean) =>
        measured.get(measureKey(block.id, width, opened)) ?? estimate(block, width, opened);
      const pinned = new Set(
        board.blocks.flatMap((block) => (pins.has(block.id) ? [block.id] : [])),
      );
      const resultBlocks = board.blocks.filter(
        (block) => !found.has(block.id) && !pinned.has(block.id),
      );
      const layout = boardLayout(
        resultBlocks.map((block): LayoutBlock => ({
          id: block.id,
          kind: block.kind,
          emphasis: block.emphasis,
          ...(block.group ? { group: block.group } : {}),
          width: widthRange(block),
          height: (width) => sized(block, width, block.id === open),
        })),
        open ? { open } : {},
      );
      const headId = board.title || board.lead || board.more ? `head:${draft.card}` : undefined;
      const headWidth = Math.min(Math.max(layout.width, BOARD.min), HEAD.width);
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
      const rows = parts.map((part) => ({
        part: { id: part.id, ...partSize(partShape(part)) },
        found: board.blocks.flatMap((block) => {
          if (found.get(block.id) !== part.id || pinned.has(block.id)) return [];
          const width = Math.round(widthRange(block).ideal);
          return [{ id: block.id, width, height: sized(block, width, block.id === open) }];
        }),
        feeds: part.state === "done" && (part.pages.length > 0 || part.sources.length > 0),
      }));
      const lane = placeRun(top, {
        request: requestPart,
        inputs: inputs.map((entry) => ({ id: entry.id, ...INPUT })),
        rows,
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
          parts: parts.map((part) => part.id),
          ...(headId ? { head: headId } : {}),
        },
        parts,
        inputs,
        found,
        targets: Object.fromEntries(
          Object.entries(lane.rects).map(([id, rect]) => [id, { x: rect.x, y: rect.y }]),
        ),
      });
      top = lane.extent + RUN.between;
    }
  }
  return stages;
}

type Box = { x: number; y: number; width: number; height: number };
const overlaps = (a: Box, b: Box) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

/**
 * The person's own things never sit under a run: one that a run has grown
 * over stands just past its right edge, at the height it had, until the
 * person moves it. Nothing here is saved.
 */
export function clearOfBands(
  positions: Readonly<Record<string, CanvasPosition>>,
  sizes: Readonly<Record<string, { width: number; height: number }>>,
  stages: readonly WorkStage[],
): Record<string, CanvasPosition> {
  const moved: Record<string, CanvasPosition> = {};
  for (const [id, at] of Object.entries(positions)) {
    const size = sizes[id] ?? { width: 280, height: 160 };
    const hit = stages.find((stage) => overlaps(stage.lane.box, { ...at, ...size }));
    moved[id] = hit ? { x: hit.lane.box.x + hit.lane.box.width + RUN.gutter * 2, y: at.y } : at;
  }
  return moved;
}

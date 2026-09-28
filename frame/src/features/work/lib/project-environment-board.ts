import type {
  WorkEnvironmentElement,
  WorkEnvironmentSnapshot,
  WorkArtifactV1,
  WorkExecutionFact,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import type { MediaAssetV1 } from "$domain/resources";
import { isLive } from "./agent-steps";
import { clipText, type CanvasPosition } from "./canvas-model";
import { threadOf, type WorkStage } from "./project-environment-thread";
import { boardOf } from "./board/adapter";
import { boardLayout, type LayoutBlock } from "./board/layout";
import { leadObject, runObjects, type RunObject } from "./board/objects";
import { objectHeight, objectWidth } from "./board/object-size";
import { ulidTime } from "./ulid-time";
import { runTrail } from "./board/trail";
import type { Detail, Picture } from "./board/types";
import { RUN, placeRun } from "./run/layout";
import { runParts, type PartAsk, type RunInputView, type RunPart } from "./run/parts";
import { PART, askKey, partSize, type PartShape } from "./run/part-size";
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

/** A block's height as it last measured itself at this width, open or not. */
export const measureKey = (id: string, width: number, open: boolean, detail: Detail = "full") =>
  `${id}|${Math.round(width)}|${open ? 1 : 0}${detail === "full" ? "" : `|${detail}`}`;

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
  /** The canvas's detail: an object surveyed from afar takes the room it draws at that detail. */
  detail?: Detail;
  /** Questions waiting on the person, by the work they belong to. */
  asks?: (objective: string) => readonly PartAsk[];
  /** What a run drew on before it began. */
  inputs?: (runs: readonly WorkExecutionFact[]) => readonly RunInputView[];
};

/** An input's mark: its glyph and its words on one line. */
const INPUT = { width: 200, height: 28 } as const;

const clock = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

/**
 * The lead's objects a run placed, each showing the newest version of its
 * chain; an object that revises one already on the canvas shows there
 * instead, and the run names it as what it changed.
 */
function leadEntries(
  snapshot: WorkEnvironmentSnapshot,
  projection: WorkRuntimeProjection,
  executions: readonly string[],
) {
  const all = new Map(
    projection.executions.flatMap((execution) =>
      execution.artifacts.map((artifact) => [artifact.id, { artifact, execution }] as const),
    ),
  );
  const newer = new Map<string, string>();
  for (const [id, { artifact }] of all) if (artifact.revises) newer.set(artifact.revises, id);
  const placed = new Map<string, string>();
  for (const element of snapshot.elements)
    if (element.reference.kind === "artifact" && element.reference.objective === projection.work.id)
      placed.set(element.reference.artifact, element.id);
  const rootOf = (id: string) => {
    let at = id;
    for (let hop = 0; hop < 64; hop++) {
      const previous = all.get(at)?.artifact.revises;
      if (!previous || !all.has(previous)) break;
      at = previous;
    }
    return at;
  };
  const newestOf = (id: string) => {
    let at = id;
    for (let hop = 0; hop < 64 && newer.has(at); hop++) at = newer.get(at)!;
    return at;
  };
  const entries: {
    element: string;
    artifact: WorkArtifactV1;
    execution: WorkExecutionFact;
    updated?: string;
  }[] = [];
  const revised: string[] = [];
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (
      reference.kind !== "artifact" ||
      reference.objective !== projection.work.id ||
      !executions.includes(reference.execution)
    )
      continue;
    const own = all.get(reference.artifact);
    if (!own || !leadObject(own.artifact)) continue;
    const root = rootOf(reference.artifact);
    const holder = root !== reference.artifact ? placed.get(root) : undefined;
    if (holder) {
      if (!revised.includes(holder)) revised.push(holder);
      continue;
    }
    const newest = all.get(newestOf(reference.artifact))!;
    const at = newest.artifact.id !== reference.artifact ? ulidTime(newest.execution.id) : null;
    entries.push({
      element: element.id,
      artifact: newest.artifact,
      execution: newest.execution,
      ...(at ? { updated: m.work_object_updated({ time: clock.format(at) }) } : {}),
    });
  }
  return { entries, revised };
}

/** How a part's own node stands: frames while it works, a stack once done. */
export function partShape(part: RunPart, measured?: number): PartShape {
  if (part.ask) {
    const ask = part.ask.props["ask"] as { kind?: string } | undefined;
    return {
      kind: "ask",
      height: measured ?? (ask?.kind === "confirm" ? PART.askConfirm : PART.askHeight),
    };
  }
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
  const detail = options.detail ?? "full";
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
          if (ask) part.ask = { props: ask.props };
        }
      const inputs = (options.inputs?.(runs) ?? []).map((input, index) => ({
        id: `input:${draft.card}:${index}`,
        input,
      }));
      const headId = `head:${draft.card}`;
      const lead = leadEntries(snapshot, projection, draft.executions);
      const set = runObjects({
        board,
        head: headId,
        live,
        pending: m.work_board_pending(),
        lead: lead.entries,
      });
      const legacyFound = foundByPart(board, parts);
      const found = new Map<string, string>();
      for (const object of set.objects) {
        const part = object.part
          ? parts.find((candidate) => candidate.key === object.part)?.id
          : legacyFound.get(object.id);
        if (part) found.set(object.id, part);
      }
      const open = options.open ?? undefined;
      const range = (object: RunObject) => objectWidth(object.view);
      const sized = (object: RunObject, width: number, opened: boolean) =>
        (detail !== "full"
          ? measured.get(measureKey(object.id, width, opened, detail))
          : undefined) ??
        measured.get(measureKey(object.id, width, opened)) ??
        objectHeight(object.view, width);
      const pinned = new Set(
        set.objects.flatMap((object) => (pins.has(object.id) ? [object.id] : [])),
      );
      const layout = boardLayout(
        set.objects
          .filter((object) => !found.has(object.id) && !pinned.has(object.id))
          .map((object): LayoutBlock => ({
            id: object.id,
            kind: object.view.kind,
            emphasis: object.emphasis,
            ...(object.block?.group ? { group: object.block.group } : {}),
            width: range(object),
            height: (width) => sized(object, width, object.id === open),
          })),
        open ? { open } : {},
      );
      const reply = set.reply;
      const headWidth = reply
        ? Math.max(objectWidth(reply.view).min, Math.min(layout.width, objectWidth(reply.view).max))
        : 0;
      const head = reply
        ? { id: reply.id, width: headWidth, height: sized(reply, headWidth, false) }
        : undefined;
      const pinSizes = new Map(
        set.objects.flatMap((object) => {
          const pin = pins.get(object.id);
          if (!pin) return [];
          const width = range(object).ideal;
          return [[object.id, { ...pin, width, height: sized(object, width, false) }] as const];
        }),
      );
      const rows = parts.map((part) => ({
        part: {
          id: part.id,
          ...partSize(
            partShape(
              part,
              (detail !== "full"
                ? measured.get(measureKey(askKey(part.id), PART.ask, false, detail))
                : undefined) ?? measured.get(measureKey(askKey(part.id), PART.ask, false)),
            ),
          ),
        },
        found: set.objects.flatMap((object) => {
          if (found.get(object.id) !== part.id || pinned.has(object.id)) return [];
          const width = Math.round(range(object).ideal);
          return [{ id: object.id, width, height: sized(object, width, object.id === open) }];
        }),
        feeds:
          part.state === "done" &&
          (part.pages.length > 0 || part.sources.length > 0 || !!part.lines?.length),
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
          ...(head ? { head: head.id } : {}),
        },
        objects: set.objects,
        ...(reply ? { reply } : {}),
        revised: lead.revised,
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
  /** The person's things with no place yet: they stand in a column right of every run. */
  loose: readonly string[] = [],
): Record<string, CanvasPosition> {
  const moved: Record<string, CanvasPosition> = {};
  const right = Math.max(0, ...stages.map((stage) => stage.lane.box.x + stage.lane.box.width));
  const x = stages.length ? right + RUN.gutter * 2 : 80;
  let below = 120;
  for (const id of loose) {
    if (positions[id]) continue;
    moved[id] = { x, y: below };
    below += (sizes[id]?.height ?? 160) + RUN.rowGap;
  }
  for (const [id, at] of Object.entries(positions)) {
    const size = sizes[id] ?? { width: 280, height: 160 };
    const hit = stages.find((stage) => overlaps(stage.lane.box, { ...at, ...size }));
    moved[id] = hit ? { x: hit.lane.box.x + hit.lane.box.width + RUN.gutter * 2, y: at.y } : at;
  }
  return moved;
}

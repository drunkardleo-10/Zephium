import { measure } from "./diagram-text";
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
import {
  threadOf,
  type RunTurn,
  type RunTurns,
  type WorkStage,
} from "./project-environment-thread";
import { boardOf } from "./board/adapter";
import { boardLayout, type LayoutBlock } from "./board/layout";
import { leadObject, onCanvas, pictureKey, runObjects, type RunObject } from "./board/objects";
import { objectHeight, objectWidth } from "./board/object-size";
import { ulidTime } from "./ulid-time";
import { runTrail } from "./board/trail";
import type { Picture } from "./board/types";
import { RUN, placeRun } from "./run/layout";
import { localReads, runParts, type PartAsk, type RunInputView, type RunPart } from "./run/parts";
import { PART, partSize, rowKey, type PartShape } from "./run/part-size";
import { runSources } from "./run/sources";
import { computerRows, computerView } from "./parts/computer";
import { foundByPart } from "./run/found";
import { provenance } from "./run/provenance";
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
 * Pictures the canvas admitted from the web, by the address they were fetched
 * from: an object's item finds its own among its candidates.
 */
export function fetchedPictures(
  snapshot: WorkEnvironmentSnapshot,
  media: ReadonlyMap<string, MediaAssetV1>,
): Map<string, Picture> {
  const used = new Set(
    (snapshot.relations ?? []).flatMap((relation) =>
      relation.kind === "uses" ? [relation.to] : [],
    ),
  );
  const pictures = new Map<string, Picture>();
  for (const element of snapshot.elements) {
    if (element.reference.kind !== "resource" || !used.has(element.id)) continue;
    const asset = media.get(element.reference.resource);
    if (asset?.kind !== "image" || asset.origin?.kind !== "fetched") continue;
    pictures.set(pictureKey(asset.origin.url), { profile: snapshot.profile, digest: asset.digest });
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
export const measureKey = (id: string, width: number, open: boolean) =>
  `${id}|${Math.round(width)}|${open ? 1 : 0}`;

/**
 * What blocks report of their height, taken in once per turn: every block
 * that measured itself while the canvas drew lands in one change, so the runs
 * are laid out again once, not once per block.
 */
export function measurer(measured: Map<string, number>) {
  const pending = new Map<string, number>();
  let queued = false;
  return (id: string, width: number, open: boolean, height: number) => {
    pending.set(measureKey(id, width, open), height);
    if (queued) return;
    queued = true;
    queueMicrotask(() => {
      queued = false;
      for (const [key, value] of pending) if (measured.get(key) !== value) measured.set(key, value);
      pending.clear();
    });
  };
}

export type StageOptions = {
  recorded?: (objective: string) => readonly WorkPageV1[];
  pictures?: ReadonlyMap<string, Picture>;
  /** Pictures admitted from the web, by the address they came from. */
  fetched?: ReadonlyMap<string, Picture>;
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
  /** The questions a run put to the person and the answers, as the asks read them. */
  exchange?: (runs: readonly WorkExecutionFact[]) => RunTurn[];
};

/** An input's mark: its name over what kind of thing it is, its tile where its line leaves. */
const INPUT = { width: 200, height: 36 } as const;

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
    if (!own || !leadObject(own.artifact) || !onCanvas(own.artifact, own.execution)) continue;
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

type InputFact = NonNullable<WorkExecutionFact["inputs"]>[number];
const baseName = (path: string) => path.replace(/\/+$/u, "").split("/").at(-1) || path;

/**
 * The folders a run read, by name: the one its fact names, else the
 * canvas's folders its files sit in, else the canvas's folders when they are
 * as many as it read. A count is never a name.
 */
type Folder = { label: string; element?: string };
function folderNames(
  snapshot: WorkEnvironmentSnapshot,
  run: WorkExecutionFact,
  fact: InputFact,
): Folder[] {
  const folders = snapshot.elements.flatMap((element) =>
    element.reference.kind === "folder"
      ? [{ path: element.reference.path, name: element.reference.name, element: element.id }]
      : [],
  );
  const named = (folder: (typeof folders)[number]): Folder => ({
    label: folder.name || baseName(folder.path),
    element: folder.element,
  });
  if (fact.reference?.startsWith("/")) {
    const folder = folders.find((candidate) => candidate.path === fact.reference);
    return [folder ? named(folder) : { label: baseName(fact.reference) }];
  }
  const read = [
    ...(run.file_evidence ?? []).map((record) => record.file.path),
    ...(run.steps ?? []).flatMap((step) => (step.local?.folder ? [step.local.folder] : [])),
  ];
  const used = folders.filter((folder) =>
    read.some((path) => path === folder.path || path.startsWith(`${folder.path}/`)),
  );
  if (used.length) return used.map(named);
  if (folders.length && folders.length === (fact.count ?? 1)) return folders.map(named);
  return [{ label: fact.label }];
}

/** Room the lines under a request take, until they measure themselves. */
function turnsHeight(turns: RunTurns): number {
  const lines = (text: string, per: number) => Math.max(1, Math.ceil(text.length / per));
  let height = turns.local.length ? 6 + turns.local.length * 20 : 0;
  for (const turn of turns.exchange)
    height +=
      turn.kind === "ask"
        ? 12 + lines(turn.question, 40) * 18 + lines(turn.answer ?? "", 40) * 18
        : 12 + lines(turn.text, 40) * 18;
  return height + (turns.exchange.length ? 8 : 0);
}

/** Each input once, as a run's several executions and later requests of its thread name it again. */
function uniqueInputs(inputs: readonly RunInputView[], shown: Set<string>): RunInputView[] {
  return inputs.filter((input) => {
    const key = `${input.kind}:${input.label}`;
    return !shown.has(key) && !!shown.add(key);
  });
}

/** How a part's own node stands: its pages as windows, its sources, its helper's view or its ask. */
export function partShape(part: RunPart, projection?: WorkRuntimeProjection): PartShape {
  if (part.ask) {
    const ask = part.ask.props["ask"] as { kind?: string } | undefined;
    return { kind: "ask", confirm: ask?.kind === "confirm" };
  }
  if (part.helper === "computer" || part.helper === "connection") {
    const steps = part.steps ?? [];
    const rows =
      part.helper === "computer"
        ? computerRows(computerView(projection, steps))
        : // A call a step, four shown and a line for the rest, until the view measures itself.
          Math.max(1, Math.min(steps.length, 4) + (steps.length > 4 ? 1 : 0));
    return {
      kind: "helper",
      rows: Math.max(rows, Math.min(PART.helperLines, part.lines?.length ?? 0)),
    };
  }
  if (part.helper === "research" && !part.pages.length)
    return {
      kind: "sources",
      rows: part.sources.length,
      more: part.sources.length > PART.sourceRows,
    };
  if (!part.pages.length) return { kind: "label" };
  return { kind: "pages", count: part.pages.length };
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
    /** What earlier requests of this thread already showed they drew on. */
    const shown = new Set<string>();
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
      const opened = !!options.requests?.has(draft.card);
      const turns: RunTurns = { local: localReads(runs), exchange: options.exchange?.(runs) ?? [] };
      const words = requestTextSize(request, opened);
      // Where the words' first line ends, so the run's lines leave from them, not from past them.
      const said = request.trim();
      const single = measure(said, 17, 500);
      const requestPart = {
        id: draft.card,
        width: words.width,
        ...(single < RUN.request - 8 ? { end: Math.ceil(single) + 8 } : {}),
        height:
          measured.get(measureKey(draft.card, RUN.request, opened)) ??
          words.height + turnsHeight(turns),
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
      // What the run drew on: the lead records it as facts; a caller may add its own.
      const folders: string[] = [];
      const recordedInputs = uniqueInputs(
        runs.flatMap((run) =>
          (run.inputs ?? []).flatMap((fact): RunInputView[] =>
            fact.kind === "files"
              ? folderNames(snapshot, run, fact).map((folder) => {
                  if (folder.element) folders.push(folder.element);
                  return {
                    kind: "files",
                    label: folder.label,
                    lit: true,
                    folder: true,
                    ...(folder.element ? { element: folder.element } : {}),
                  };
                })
              : [
                  {
                    kind: fact.kind,
                    label: fact.label,
                    lit: true,
                    ...(fact.count ? { count: fact.count } : {}),
                  },
                ],
          ),
        ),
        shown,
      );
      const inputs = (options.inputs?.(runs) ?? recordedInputs).map((input, index) => ({
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
        ...(options.fetched ? { fetched: options.fetched } : {}),
      });
      const legacyFound = foundByPart(board, parts);
      const found = new Map<string, string>();
      for (const object of set.objects) {
        const part = object.part
          ? parts.find((candidate) => candidate.key === object.part)?.id
          : legacyFound.get(object.id);
        if (part) found.set(object.id, part);
      }
      // A research part's notes (a list of what it read, not the person's own to-dos or
      // messages) are what it handed the lead, not something it found: its row keeps its
      // summary and the notes live in the part's opened view.
      const notes = new Map<string, RunObject[]>();
      const folded = new Set<string>();
      for (const object of set.objects) {
        const holder = found.get(object.id);
        const part = holder ? parts.find((candidate) => candidate.id === holder) : undefined;
        const view = object.view;
        if (
          part?.helper !== "research" ||
          view.kind !== "list" ||
          view.style === "todo" ||
          view.style === "messages" ||
          view.items.some((item) => !!item.from?.who || !!item.from?.app)
        )
          continue;
        folded.add(object.id);
        found.delete(object.id);
        notes.set(part.id, [...(notes.get(part.id) ?? []), object]);
      }
      // What a part found, joined to the pages it was taken from: those pages stand over
      // what they gave, not in the part's stack.
      const sourced = new Map<string, Set<string>>();
      const objects = set.objects.map((object) => {
        const holder = found.get(object.id);
        const part = holder ? parts.find((candidate) => candidate.id === holder) : undefined;
        if (!part || object.view.kind !== "picks") return object;
        const told = provenance(object.view, part.pages, runs, recorded);
        if (!told) return object;
        sourced.set(part.id, new Set([...(sourced.get(part.id) ?? []), ...told.used]));
        return { ...object, view: told.view };
      });
      for (const part of parts) {
        const used = sourced.get(part.id);
        if (used) part.pages = part.pages.filter((group) => !used.has(group.id));
      }
      const open = options.open ?? undefined;
      const range = (object: RunObject) => objectWidth(object.view);
      const sized = (object: RunObject, width: number, opened: boolean) =>
        measured.get(measureKey(object.id, width, opened)) ?? objectHeight(object.view, width);
      const pinned = new Set(objects.flatMap((object) => (pins.has(object.id) ? [object.id] : [])));
      const layout = boardLayout(
        objects
          .filter(
            (object) => !found.has(object.id) && !pinned.has(object.id) && !folded.has(object.id),
          )
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
        objects.flatMap((object) => {
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
            partShape(part, projection),
            measured.get(measureKey(rowKey(part.id), 0, false)),
          ),
        },
        found: objects.flatMap((object) => {
          if (found.get(object.id) !== part.id || pinned.has(object.id)) return [];
          const width = Math.round(Math.min(FOUND, range(object).ideal));
          return [{ id: object.id, width, height: sized(object, width, object.id === open) }];
        }),
        feeds:
          part.state === "done" &&
          (part.pages.length > 0 || part.sources.length > 0 || !!part.lines?.length),
      }));
      const drawn = runSources(runs, recorded);
      const sourcesId = `sources:${draft.card}`;
      // Sources hold what a run drew on; a block that would only report a failure is not drawn.
      const sources = drawn.rows.length
        ? {
            id: sourcesId,
            width: SOURCES,
            height: measured.get(measureKey(sourcesId, SOURCES, false)) ?? sourcesHeight(drawn),
          }
        : undefined;
      const lane = placeRun(top, {
        request: requestPart,
        ...(sources ? { sources } : {}),
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
        objects: objects,
        ...(reply ? { reply } : {}),
        revised: lead.revised,
        parts,
        inputs,
        found,
        ...(sources ? { sources: { id: sourcesId, view: drawn } } : {}),
        notes,
        turns,
        folders,
        targets: Object.fromEntries(
          Object.entries(lane.rects).map(([id, rect]) => [id, { x: rect.x, y: rect.y }]),
        ),
      });
      top = lane.extent + RUN.between;
    }
  }
  return stages;
}

/** What a part found stands at its row's end no wider than this, so the result keeps the view. */
const FOUND = 760;
/** The Sources under a result: a reading column of rows. */
const SOURCES = 560;
/** Rows the Sources show before Show all. */
export const SOURCE_ROWS = 6;
function sourcesHeight(drawn: ReturnType<typeof runSources>): number {
  const frames = drawn.rows.some((row) => row.frame) ? 84 : 0;
  const rows = Math.min(SOURCE_ROWS, drawn.rows.length);
  const more = drawn.rows.length > SOURCE_ROWS ? 32 : 0;
  return 28 + frames + rows * 32 + more + (drawn.unread.length ? 36 : 0);
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

import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import { clipText, type CanvasItem, type CanvasLink, type CanvasPosition } from "./canvas-model";
import type { Board } from "./board/types";
import type { Rect } from "./board/layout";
import type { TrailLine } from "./board/trail";
import { RUN, type RunPlace } from "./run/layout";
import type { RunInputView, RunPart } from "./run/parts";
import type { RunObject } from "./board/objects";
import * as m from "$shared/i18n/messages";

/**
 * One message from the person and everything it set going: a lane. The first
 * message of a work is its objective element; every later one gets a request
 * card of its own, so the whole thread stays readable down the canvas.
 */
export type WorkStage = {
  /** The objective element that owns the thread. */
  element: string;
  objective: string;
  /** The request this message's run hangs from. */
  card: string;
  /** The person's sentence. */
  request: string;
  /** The executions this message started, oldest first. */
  executions: string[];
  /** Whether one of the run's executions is still going. */
  live: boolean;
  /** Where the request stands. */
  place: Rect;
  board: Board;
  lane: RunPlace;
  /** What the runs did, as closed facts. */
  trail: TrailLine[];
  /** The run's nodes by role: its parts in row order, the result's head. */
  column: { parts: string[]; head?: string };
  /** One row per part, in the order their work began. */
  parts: RunPart[];
  /** What the run drew on, left of its request. */
  inputs: { id: string; input: RunInputView }[];
  /** Everything the run made, the reply apart. */
  objects: RunObject[];
  /** The answer at the head of the result. */
  reply?: RunObject;
  /** Objects of earlier runs this one revised: its request draws a line to each. */
  revised: string[];
  /** The part each found block belongs to; the rest are the result's. */
  found: ReadonlyMap<string, string>;
  /** Blocks the person moved out of the run's flow. */
  pinned: ReadonlySet<string>;
  /** Where every node of the run stands. */
  targets: Record<string, CanvasPosition>;
};
const REQUEST_TEXT = 512;

/** The sentence a work began with; the objective element keeps saying it. */
export function firstRequest(projection: WorkRuntimeProjection): string {
  return projection.executions[0]?.spec.request?.trim() || projection.work.objective;
}

/**
 * A work's messages as lanes: a new sentence opens one, running the same one
 * again continues it. The first lane hangs from the objective element.
 */
export function threadOf(
  element: string,
  projection: WorkRuntimeProjection,
): { card: string; request: string; executions: string[] }[] {
  const thread: { card: string; request: string; executions: string[] }[] = [];
  for (const [order, execution] of projection.executions.entries()) {
    const request = clipText(execution.spec.request?.trim() ?? "", REQUEST_TEXT);
    const current = thread.at(-1);
    if (!current)
      thread.push({
        card: element,
        request: clipText(firstRequest(projection), REQUEST_TEXT),
        executions: [execution.id],
      });
    else if (order > 0 && request && request !== current.request)
      thread.push({
        card: `request:${element}:${execution.id}`,
        request,
        executions: [execution.id],
      });
    else current.executions.push(execution.id);
  }
  if (!thread.length)
    thread.push({
      card: element,
      request: clipText(firstRequest(projection), REQUEST_TEXT),
      executions: [],
    });
  return thread;
}

/**
 * The request of every message after the first, the thread into it from the
 * one before, and where every node of every run stands.
 */
export function environmentRequests(stages: readonly WorkStage[]): {
  items: CanvasItem[];
  links: CanvasLink[];
  positions: Record<string, CanvasPosition>;
} {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
  const positions: Record<string, CanvasPosition> = {};
  for (const [index, stage] of stages.entries()) {
    Object.assign(positions, stage.targets);
    links.push(...runLinks(stage));
    links.push(...reviseLinks(stage, stages.slice(0, index)));
    const previous = stages[index - 1];
    if (!previous || previous.element !== stage.element) continue;
    items.push({
      id: stage.card,
      type: "request",
      kind: m.work_env_request(),
      title: stage.request,
      detail: "",
      status: "",
    });
    const above = previous.place;
    const below = stage.place;
    const x = THREAD_X;
    links.push({
      id: `stage:${stage.card}`,
      source: previous.card,
      target: stage.card,
      kind: "thread",
      route: {
        points: [
          { x, y: above.y + above.height + 40 },
          { x, y: below.y - 12 },
        ],
        from: { x, y: above.height + 40 },
        to: { x, y: -12 },
        laid: { source: { x: above.x, y: above.y }, target: { x: below.x, y: below.y } },
      },
    });
  }
  return { items, links, positions };
}

/**
 * A follow-up's line to each object it revised in an earlier run: along its
 * spine, up the gutter just left of the object, into its top-left corner.
 */
function reviseLinks(stage: WorkStage, earlier: readonly WorkStage[]): CanvasLink[] {
  const spine = stage.place.y + RUN.spine;
  return stage.revised.flatMap((target): CanvasLink[] => {
    const rect = earlier.find((entry) => entry.lane.rects[target])?.lane.rects[target];
    if (!rect) return [];
    const start = { x: RUN.request + RUN.air, y: spine };
    const x = rect.x - RUN.found / 2;
    const end = { x: rect.x - RUN.air, y: rect.y + RUN.labelMid };
    return [
      {
        id: `revise:${stage.card}:${target}`,
        source: stage.card,
        target,
        kind: "thread",
        route: {
          points: [start, { x, y: spine }, { x, y: end.y }, end],
          from: { x: start.x - stage.place.x, y: RUN.spine },
          to: { x: -RUN.air, y: RUN.labelMid },
          laid: {
            source: { x: stage.place.x, y: stage.place.y },
            target: { x: rect.x, y: rect.y },
          },
        },
      },
    ];
  });
}

/** The thread runs down the request column's left edge, under the words. */
const THREAD_X = 10;

/**
 * A run's lines: the request into each part's row, each done part's row into
 * the result, or the request straight into the result when it has no parts.
 * A part's line carries work while the part is live.
 */
function runLinks(stage: WorkStage): CanvasLink[] {
  const live = new Set(stage.parts.flatMap((part) => (part.state === "running" ? [part.id] : [])));
  return stage.lane.lines.map((line) => ({
    id: line.id,
    source: line.source,
    target: line.target,
    kind: "flow",
    route: { points: line.points, from: line.from, to: line.to, laid: line.laid },
    ...(line.kind === "part" && (live.has(line.target) || (!stage.parts.length && stage.live))
      ? { live: true }
      : {}),
  }));
}

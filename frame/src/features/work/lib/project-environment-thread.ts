import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import { clipText, type CanvasItem, type CanvasLink, type CanvasPosition } from "./canvas-model";
import type { Board } from "./board/types";
import type { LanePlace } from "./board/lane";
import type { Rect } from "./board/layout";
import type { TrailLine } from "./board/trail";
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
  /** The card this message's work hangs from. */
  card: string;
  /** The person's sentence. */
  request: string;
  /** The executions this message started, oldest first. */
  executions: string[];
  /** Whether one of the lane's runs is still going. */
  live: boolean;
  /** Where the request card stands, at the top of the process column. */
  place: Rect;
  board: Board;
  lane: LanePlace;
  /** What the runs did, as closed facts. */
  trail: TrailLine[];
  /** The process column's other cards and the board's head, by id. */
  column: { trail?: string; pages: string[]; sources?: string; head?: string };
  /** Blocks the person moved out of the board's flow. */
  pinned: ReadonlySet<string>;
  /** Where every card of the lane stands. */
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
 * The request card of every message after the first, the thread into it, and
 * where every card of every lane stands: the person's words join request to
 * request down the process column.
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
    links.push({
      id: `stage:${stage.card}`,
      source: previous.card,
      target: stage.card,
      kind: "thread",
    });
  }
  return { items, links, positions };
}

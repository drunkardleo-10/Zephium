import type { WorkEnvironmentSnapshot, WorkRuntimeProjection } from "$shared/ipc/bindings";
import {
  clipText,
  type CanvasItem,
  type CanvasLink,
  type CanvasPosition,
  type CanvasSize,
} from "./canvas-model";
import { SOURCES_SIZE } from "./organize";
import * as m from "$shared/i18n/messages";

/**
 * One message from the person and everything it set going. The first message
 * of a work is its objective element; every later one gets a request card of
 * its own, so the whole thread stays readable on the canvas.
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
  /** Where the card stands; what the run places lands beneath it. */
  place: CanvasPosition & CanvasSize;
};
const REQUEST_TEXT = 512;
const REQUEST_SIZE = { width: 320, height: 150 } as const;
/** The air between one stage and the next request that follows it. */
const STAGE_GAP = 72;
/** The drop from a request card to the Sources card of its run. */
export const STAGE_DROP = 48;

/** The sentence a work began with; the objective element keeps saying it. */
export function firstRequest(projection: WorkRuntimeProjection): string {
  return projection.executions[0]?.spec.request?.trim() || projection.work.objective;
}

/** The stage each execution belongs to, laid out down the canvas in order. */
export function environmentStages(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
): WorkStage[] {
  const stages: WorkStage[] = [];
  for (const [index, element] of snapshot.elements.entries()) {
    if (element.reference.kind !== "objective") continue;
    const projection = objectives.get(element.reference.objective);
    if (!projection) continue;
    const place = snapshot.view.placements.find((place) => place.element === element.id) ?? {
      x: 80 + (index % 3) * 520,
      y: 120 + Math.floor(index / 3) * 420,
      ...REQUEST_SIZE,
    };
    let stage: WorkStage = {
      element: element.id,
      objective: projection.work.id,
      card: element.id,
      request: clipText(firstRequest(projection), REQUEST_TEXT),
      executions: [],
      place: { x: place.x, y: place.y, width: place.width, height: place.height },
    };
    stages.push(stage);
    for (const [order, execution] of projection.executions.entries()) {
      const request = clipText(execution.spec.request?.trim() ?? "", REQUEST_TEXT);
      // A new sentence opens a stage; running the same one again continues it.
      if (order > 0 && request && request !== stage.request) {
        stage = {
          element: element.id,
          objective: projection.work.id,
          card: `request:${element.id}:${execution.id}`,
          request,
          executions: [],
          place: { x: place.x, y: extent(snapshot, stage) + STAGE_GAP, ...REQUEST_SIZE },
        };
        stages.push(stage);
      }
      stage.executions.push(execution.id);
    }
  }
  return stages;
}

/** How far down a stage reaches: its card, the Sources band under it, and
 * whatever its runs have already placed. */
function extent(snapshot: WorkEnvironmentSnapshot, stage: WorkStage): number {
  let bottom = stage.place.y + stage.place.height + STAGE_DROP + SOURCES_SIZE.height;
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (!("execution" in reference) || !stage.executions.includes(reference.execution)) continue;
    const place = snapshot.view.placements.find((place) => place.element === element.id);
    if (place) bottom = Math.max(bottom, place.y + place.height);
  }
  return bottom;
}

/** The request card of every message after the first, and the path into it. */
export function environmentRequests(
  snapshot: WorkEnvironmentSnapshot,
  stages: readonly WorkStage[],
  /** The Sources cards the scene has, so a path can leave from one. */
  sources: ReadonlySet<string>,
): { items: CanvasItem[]; links: CanvasLink[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
  const positions: Record<string, CanvasPosition> = {};
  for (const [index, stage] of stages.entries()) {
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
    positions[stage.card] = { x: stage.place.x, y: stage.place.y };
    links.push({
      id: `stage:${stage.card}`,
      source: lastCard(snapshot, previous, sources),
      target: stage.card,
      kind: "dependency",
    });
  }
  return { items, links, positions };
}

/** Where a stage ends: its result, else its Sources card, else its request. */
function lastCard(
  snapshot: WorkEnvironmentSnapshot,
  stage: WorkStage,
  sources: ReadonlySet<string>,
): string {
  let result = "";
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (reference.kind === "artifact" && stage.executions.includes(reference.execution))
      result = element.id;
  }
  if (result) return result;
  for (const execution of [...stage.executions].reverse()) {
    const id = `sources:${stage.element}:${execution}`;
    if (sources.has(id)) return id;
  }
  return stage.card;
}

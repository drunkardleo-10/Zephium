import type {
  WorkEnvironmentSnapshot,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import {
  clipText,
  type CanvasItem,
  type CanvasLink,
  type CanvasPosition,
  type CanvasSize,
} from "./canvas-model";
import { requestSize } from "./card-size";
import { stageContents } from "./project-environment-stage";
import { STAGE_GAP, stageLayout, type StageContents, type StageLayout } from "./stage-layout";
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
  /** Where the card stands; the stage's clusters read rightwards from it. */
  place: CanvasPosition & CanvasSize;
  /** What each cluster holds, and where those cards stand. */
  contents: StageContents;
  layout: StageLayout;
};
const REQUEST_TEXT = 512;

/** The sentence a work began with; the objective element keeps saying it. */
export function firstRequest(projection: WorkRuntimeProjection): string {
  return projection.executions[0]?.spec.request?.trim() || projection.work.objective;
}

/** The stage each execution belongs to: stages hang down the canvas, each
 * request under the tallest cluster of the stage above it. */
export function environmentStages(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  /** The pages each work recorded, so a stage counts its page cards as they are drawn. */
  recorded: (objective: string) => readonly WorkPageV1[] = () => [],
): WorkStage[] {
  const stages: WorkStage[] = [];
  for (const [index, element] of snapshot.elements.entries()) {
    if (element.reference.kind !== "objective") continue;
    const projection = objectives.get(element.reference.objective);
    if (!projection) continue;
    const saved = snapshot.view.placements.find((place) => place.element === element.id);
    const first = saved
      ? { x: saved.x, y: saved.y, width: saved.width, height: saved.height }
      : {
          x: 80 + (index % 3) * 520,
          y: 120 + Math.floor(index / 3) * 420,
          ...requestSize(firstRequest(projection)),
        };
    const drafts: { card: string; request: string; executions: string[] }[] = [];
    for (const [order, execution] of projection.executions.entries()) {
      const request = clipText(execution.spec.request?.trim() ?? "", REQUEST_TEXT);
      const current = drafts.at(-1);
      // A new sentence opens a stage; running the same one again continues it.
      if (!current)
        drafts.push({
          card: element.id,
          request: clipText(firstRequest(projection), REQUEST_TEXT),
          executions: [execution.id],
        });
      else if (order > 0 && request && request !== current.request)
        drafts.push({
          card: `request:${element.id}:${execution.id}`,
          request,
          executions: [execution.id],
        });
      else current.executions.push(execution.id);
    }
    if (!drafts.length)
      drafts.push({
        card: element.id,
        request: clipText(firstRequest(projection), REQUEST_TEXT),
        executions: [],
      });
    const pages = recorded(projection.work.id);
    let place = first;
    for (const [order, draft] of drafts.entries()) {
      if (order > 0) place = { ...place, ...requestSize(draft.request) };
      const contents = stageContents(
        snapshot,
        projection,
        { element: element.id, executions: draft.executions },
        pages,
      );
      const layout = stageLayout(place, contents);
      stages.push({
        element: element.id,
        objective: projection.work.id,
        ...draft,
        place,
        contents,
        layout,
      });
      place = { x: first.x, y: layout.extent + STAGE_GAP, width: 0, height: 0 };
    }
  }
  return stages;
}

/**
 * The request card of every message after the first, and the thread into it:
 * the person's words join request to request; the stage between them already
 * reads from each request to its result.
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
      source: previous.card,
      target: stage.card,
      kind: "thread",
    });
  }
  return { items, links, positions };
}

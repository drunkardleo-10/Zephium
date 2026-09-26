import type {
  WorkEnvironmentSnapshot,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { isLive } from "./agent-steps";
import {
  clipText,
  type CanvasItem,
  type CanvasLink,
  type CanvasPosition,
  type CanvasSize,
} from "./canvas-model";
import { requestSize } from "./card-size";
import { laneFacts, laneOffsets, stageContents } from "./project-environment-stage";
import {
  laneShape,
  laneSlots,
  nextLane,
  placeLane,
  type StageContents,
  type StageLayout,
} from "./stage-layout";
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
  /** Where the request card stands: the lane's anchor, in the request column. */
  place: CanvasPosition & CanvasSize;
  /** What each group holds. */
  contents: StageContents;
  layout: StageLayout;
  /** Canvas-wide slot starts, shared by every lane. */
  slots: readonly number[];
  /** Whether one of the lane's runs is still going. */
  live: boolean;
  /** Where the lane's saved cards stand: their lane place plus the person's offset. */
  targets: Record<string, CanvasPosition>;
  /** What the request card says under its words. */
  facts: Pick<CanvasItem, "elapsed" | "counts" | "accounts">;
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
 * Every lane of the canvas, top to bottom: each work's messages in order,
 * requests in the column at x = 0, groups in the slots the whole canvas
 * shares, lanes 96 px apart.
 */
export function environmentStages(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  /** The pages each work recorded, so a lane counts its page cards as they are drawn. */
  recorded: (objective: string) => readonly WorkPageV1[] = () => [],
): WorkStage[] {
  const offsets = laneOffsets(snapshot);
  const drafts: {
    element: string;
    projection: WorkRuntimeProjection;
    card: string;
    request: string;
    executions: string[];
    contents: StageContents;
    size: CanvasSize;
    facts: Pick<CanvasItem, "elapsed" | "counts" | "accounts">;
  }[] = [];
  for (const element of snapshot.elements) {
    if (element.reference.kind !== "objective") continue;
    const projection = objectives.get(element.reference.objective);
    if (!projection) continue;
    const thread = threadOf(element.id, projection);
    const pages = recorded(projection.work.id);
    for (const draft of thread) {
      const facts = laneFacts(
        draft.executions.flatMap((id) => projection.executions.filter((entry) => entry.id === id)),
        pages,
      );
      drafts.push({
        element: element.id,
        projection,
        ...draft,
        contents: stageContents(
          snapshot,
          projection,
          { element: element.id, executions: draft.executions },
          pages,
          offsets,
        ),
        // A request is as tall as its words: a size saved by an older placement never clips them.
        size: requestSize(draft.request, {
          footer: !!facts.counts,
          accounts: facts.accounts?.length ?? 0,
        }),
        facts,
      });
    }
  }
  const shapes = drafts.map((draft) => laneShape(draft.contents));
  const slots = laneSlots(shapes);
  const stages: WorkStage[] = [];
  let top = 0;
  for (const [index, draft] of drafts.entries()) {
    const place = { x: 0, y: top, ...draft.size };
    const layout = placeLane(place, shapes[index]!, slots);
    const targets: Record<string, CanvasPosition> = {};
    for (const [id, base] of [[draft.card, place] as const, ...Object.entries(layout.positions)]) {
      const offset = offsets.get(id)?.offset;
      targets[id] = { x: base.x + (offset?.x ?? 0), y: base.y + (offset?.y ?? 0) };
    }
    stages.push({
      element: draft.element,
      objective: draft.projection.work.id,
      card: draft.card,
      request: draft.request,
      executions: draft.executions,
      place,
      contents: draft.contents,
      layout,
      slots,
      live: draft.executions.some((id) => {
        const execution = draft.projection.executions.find((entry) => entry.id === id);
        return !!execution && isLive(draft.projection, execution);
      }),
      targets,
      facts: draft.facts,
    });
    top = nextLane(layout);
  }
  return stages;
}

/**
 * The request card of every message after the first, the thread into it, and
 * where every saved card of every lane stands: the person's words join
 * request to request down the left margin.
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
      ...stage.facts,
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

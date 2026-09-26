import type {
  WorkElementPlacement,
  WorkExecutionFact,
  WorkFileEvidenceV1,
  WorkEnvironmentSnapshot,
  TabView,
  NoteSummary,
  WorkHumanPageV1,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { activityLabel, artifactView } from "./project-work";
import { agentDoing, isAgentExecution, isLive, type AgentDoing } from "./agent-steps";
import { subjectFacts, subjectKey, subjectsOf } from "./subjects";
import { firstRequest, type WorkStage } from "./project-environment-thread";
import {
  host,
  observedTitle,
  pageGroups,
  sourceRows,
  unreadPages,
  type SourceRow,
} from "./project-environment-stage";
import { BOARD_PIN, FRAMES, laneElement, stageRuns } from "./project-environment-board";
import { standBeside } from "./board/lane";
import type { Rect } from "./board/layout";
import { fileName } from "./work-files";
import { heldPage, humanPage, phaseLabel } from "./work-human";
import { pageFrameUrl } from "$domain/resources";
import {
  clipText,
  type CanvasItem,
  type CanvasLink,
  type CanvasPosition,
  type CanvasView,
} from "./canvas-model";

/** How many cited pages one Sources card lists; the lift shows the rest. */
const SOURCE_ROWS = 24;
/** What a card may carry: Rust admits far longer prose than a card can hold. */
const TITLE_TEXT = 512;
const DETAIL_TEXT = 2048;
const ROW_TEXT = 200;
const STATUS_TEXT = 256;
import type { MediaAssetV1 } from "$domain/resources";
import * as m from "$shared/i18n/messages";

/**
 * Media elements a subject or a link admitted its picture into: Rust records
 * the origin as a `uses` relation from that element to the media element. The
 * picture belongs to its card and never becomes a card of its own.
 */
function subjectPictures(snapshot: WorkEnvironmentSnapshot): Set<string> {
  const subjects = new Set<string>();
  const resources = new Set<string>();
  for (const element of snapshot.elements) {
    if (element.reference.kind === "subject" || element.reference.kind === "link")
      subjects.add(element.id);
    else if (element.reference.kind === "resource") resources.add(element.id);
  }
  const pictures = new Set<string>();
  for (const relation of snapshot.relations ?? [])
    if (relation.kind === "uses" && subjects.has(relation.from) && resources.has(relation.to))
      pictures.add(relation.to);
  return pictures;
}

/** The admitted picture of each subject on this canvas, by merge key, so a
 * comparison column shows the same picture as the subject's own card. */
export function environmentPictures(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  media: ReadonlyMap<string, MediaAssetV1>,
): Map<string, { profile: string; digest: string }> {
  const resources = new Map(
    snapshot.elements.flatMap((element) =>
      element.reference.kind === "resource" ? [[element.id, element.reference.resource]] : [],
    ),
  );
  const pictures = new Map<string, { profile: string; digest: string }>();
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (reference.kind !== "subject") continue;
    const execution = objectives
      .get(reference.objective)
      ?.executions.find((execution) => execution.id === reference.execution);
    const artifact = execution?.artifacts.find((artifact) => artifact.id === reference.artifact);
    const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
    if (!subject || pictures.has(subjectKey(subject))) continue;
    for (const relation of snapshot.relations ?? []) {
      if (relation.kind !== "uses" || relation.from !== element.id) continue;
      const resource = resources.get(relation.to);
      const asset = resource ? media.get(resource) : undefined;
      if (asset?.kind !== "image") continue;
      pictures.set(subjectKey(subject), { profile: snapshot.profile, digest: asset.digest });
      break;
    }
  }
  return pictures;
}

export function environmentItems(
  snapshot: WorkEnvironmentSnapshot,
  tabs: readonly TabView[],
  notes: readonly NoteSummary[],
  objectives: ReadonlyMap<string, WorkRuntimeProjection> = new Map(),
  media: ReadonlyMap<string, MediaAssetV1> = new Map(),
): CanvasItem[] {
  const decisions = new Map((snapshot.decisions ?? []).map((d) => [d.element, d.choice]));
  const resources = new Map(
    snapshot.elements.flatMap((element) =>
      element.reference.kind === "resource" ? [[element.id, element.reference.resource]] : [],
    ),
  );
  // A `uses` relation to an admitted image gives a subject its picture.
  const pictures = new Map<string, { profile: string; digest: string }>();
  for (const relation of snapshot.relations ?? []) {
    if (relation.kind !== "uses" || pictures.has(relation.from)) continue;
    const resource = resources.get(relation.to);
    const asset = resource ? media.get(resource) : undefined;
    if (asset?.kind === "image")
      pictures.set(relation.from, { profile: snapshot.profile, digest: asset.digest });
  }
  return elementItems(snapshot, tabs, notes, objectives, media).map((item) => {
    const decision = decisions.get(item.id);
    const image = pictures.get(item.id);
    return {
      ...item,
      ...(decision === undefined ? {} : { decision }),
      ...(image ? { image } : {}),
    };
  });
}

function elementItems(
  snapshot: WorkEnvironmentSnapshot,
  tabs: readonly TabView[],
  notes: readonly NoteSummary[],
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  media: ReadonlyMap<string, MediaAssetV1>,
): CanvasItem[] {
  const pictures = subjectPictures(snapshot);
  return snapshot.elements.flatMap((element) => {
    if (pictures.has(element.id)) return [];
    // What a request's runs placed is drawn by its board, not as a card of its own.
    if (element.reference.kind !== "objective" && laneElement(snapshot, element)) return [];
    const area = snapshot.areas.find((area) => area.id === element.area)?.title ?? "";
    if (element.reference.kind === "browser") {
      const tabId = element.reference.tab;
      const tab = tabs.find((tab) => tab.id === tabId);
      let origin = "";
      try {
        if (tab?.url) {
          const parsed = new URL(tab.url);
          if (parsed.origin !== "null") origin = parsed.origin;
        }
      } catch {
        /* Invalid projection text stays inert. */
      }
      return {
        id: element.id,
        type: "tab",
        area: element.area,
        kind: m.work_env_browser_resource(),
        title: tab?.title || m.work_env_unavailable_tab(),
        detail: origin,
        status: tab ? area : m.work_env_tab_unavailable(),
        icon: tab?.icon ?? null,
        unavailable: !tab,
      };
    }
    if (element.reference.kind === "resource") {
      const resourceId = element.reference.resource;
      const asset = media.get(resourceId);
      if (asset) {
        return {
          id: element.id,
          type: "media",
          area: element.area,
          kind:
            asset.kind === "image"
              ? m.work_media_kind_image()
              : asset.kind === "pdf"
                ? m.work_media_kind_pdf()
                : m.work_media_kind_file(),
          title: asset.name,
          detail: asset.mime,
          status: area,
          media: { profile: snapshot.profile, asset },
        };
      }
      const note = notes.find((note) => note.id === resourceId);
      return {
        id: element.id,
        type: "note",
        area: element.area,
        kind: m.work_env_notes(),
        title: note?.title || m.work_env_saved_resource(),
        detail: note?.modified_at ?? "",
        status: area,
        unavailable: !note,
      };
    }
    if (element.reference.kind === "folder") {
      const folder = element.reference;
      return {
        id: element.id,
        type: "folder" as const,
        area: element.area,
        kind: m.work_env_folder(),
        title: folder.name,
        detail: folder.path,
        status: area,
      };
    }
    if (element.reference.kind === "link") {
      const link = element.reference;
      let origin = "";
      try {
        origin = new URL(link.url).origin;
      } catch {
        /* Rust admits the address; an unreadable one simply has no origin line. */
      }
      return {
        id: element.id,
        type: "link" as const,
        area: element.area,
        kind: m.work_env_link(),
        // A read's observed page title names the card; the stored title stays as it is.
        title: clipText(
          observedTitle(objectives, link.url) || link.title || host(link.url) || link.url,
          TITLE_TEXT,
        ),
        detail: origin,
        status: area,
      };
    }
    const projection = objectives.get(element.reference.objective);
    // A search stage cites many pages; they appear together on one Sources card.
    if (element.reference.kind === "source") return [];
    if (element.reference.kind === "finding") return [];
    if (element.reference.kind === "subject") {
      const reference = element.reference;
      const execution = projection?.executions.find(
        (execution) => execution.id === reference.execution,
      );
      const artifact = execution?.artifacts.find((artifact) => artifact.id === reference.artifact);
      const view = artifact && execution ? artifactView(artifact, execution) : undefined;
      const subject =
        view &&
        (view.content.kind === "matrix" ||
          view.content.kind === "findings" ||
          view.content.kind === "sources")
          ? view.content.subjects[reference.index]
          : undefined;
      const facts =
        artifact && execution
          ? subjectFacts(execution, subjectsOf(artifact)[reference.index]!)
          : [];
      return {
        id: element.id,
        type: "subject",
        area: element.area,
        kind: m.work_env_subject(),
        title: clipText(subject?.name ?? m.work_artifact_unavailable(), TITLE_TEXT),
        detail: clipText(subject?.descriptor ?? "", DETAIL_TEXT),
        status: area,
        ...(subject ? { subject } : {}),
        ...(subject && facts.length ? { facts } : {}),
        unavailable: !subject,
      };
    }
    if (element.reference.kind === "artifact") {
      const reference = element.reference;
      const execution = projection?.executions.find(
        (execution) => execution.id === reference.execution,
      );
      const artifact = execution?.artifacts.find((artifact) => artifact.id === reference.artifact);
      const view = artifact && execution ? artifactView(artifact, execution) : undefined;
      return {
        id: element.id,
        type: "result",
        area: element.area,
        kind: m.work_env_result(),
        title: clipText(artifact?.title ?? m.work_artifact_unavailable(), TITLE_TEXT),
        detail: "",
        status: view?.reviewLabel ?? m.work_env_open_to_load(),
        ...(view ? { artifact: view } : {}),
        layout: "artifact",
      };
    }
    // The person's sentence, carried plainly: what the run did is its trail's.
    return {
      id: element.id,
      type: "objective",
      area: element.area,
      kind: m.work_env_request(),
      title: clipText(projection ? firstRequest(projection) : m.work_env_request(), TITLE_TEXT),
      detail: "",
      status: area,
    };
  });
}
/** What a run recorded of one file, by record id, across the canvas's works. */
export function fileEvidence(
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  record: string,
): WorkFileEvidenceV1 | undefined {
  for (const projection of objectives.values())
    for (const execution of projection.executions)
      for (const entry of execution.file_evidence ?? []) if (entry.id === record) return entry.file;
  return undefined;
}
/**
 * The saved view as the canvas reads it. A lane card's place comes from its
 * lane, so only the person's own elements bring a position and a size.
 */
export function environmentView(snapshot: WorkEnvironmentSnapshot): CanvasView {
  const lane = new Set(
    snapshot.elements.flatMap((element) => (laneElement(snapshot, element) ? [element.id] : [])),
  );
  const own = snapshot.view.placements.filter((place) => !lane.has(place.element));
  return {
    areas: Object.fromEntries(
      (snapshot.view.areas ?? []).map((place) => [
        place.area,
        { x: place.x, y: place.y, width: place.width, height: place.height },
      ]),
    ),
    sizes: Object.fromEntries(
      own.map((place) => [place.element, { width: place.width, height: place.height }]),
    ),
    positions: Object.fromEntries(own.map((place) => [place.element, { x: place.x, y: place.y }])),
    viewport: { x: snapshot.view.x, y: snapshot.view.y, zoom: snapshot.view.zoom_milli / 1000 },
  };
}

/**
 * The bounds Rust holds a checkpoint to; one card outside them refuses the
 * whole save.
 */
const PLACEMENT = { coordinate: 1_000_000, width: [120, 4096], height: [80, 4096] } as const;
const bounded = (value: number, min: number, max: number, fallback: number) =>
  Number.isFinite(value) ? Math.min(max, Math.max(min, Math.round(value))) : fallback;
function contractPlacement(place: WorkElementPlacement): WorkElementPlacement {
  const { coordinate, width, height } = PLACEMENT;
  return {
    ...place,
    x: bounded(place.x, -coordinate, coordinate, 0),
    y: bounded(place.y, -coordinate, coordinate, 0),
    width: bounded(place.width, width[0], width[1], 280),
    height: bounded(place.height, height[0], height[1], 160),
  };
}
/**
 * The placements a view saves: the person's own elements where they stand; a
 * block the person moved, as its offset from its lane's corner. Every other
 * lane element keeps what it had: its board places it.
 */
export function viewPlacements(
  snapshot: WorkEnvironmentSnapshot,
  view: CanvasView,
  stages: readonly WorkStage[],
  /** Blocks the person dragged since the canvas opened. */
  moved: ReadonlySet<string> = new Set(),
): WorkElementPlacement[] {
  const boards = new Map<string, WorkStage>();
  for (const stage of stages) for (const block of stage.board.blocks) boards.set(block.id, stage);
  return snapshot.elements.map((element) => {
    const previous = snapshot.view.placements.find((place) => place.element === element.id);
    const point = view.positions[element.id];
    const size = {
      width: view.sizes?.[element.id]?.width ?? previous?.width ?? 280,
      height: view.sizes?.[element.id]?.height ?? previous?.height ?? 160,
    };
    if (!laneElement(snapshot, element))
      return contractPlacement({
        element: element.id,
        x: point?.x ?? previous?.x ?? 0,
        y: point?.y ?? previous?.y ?? 0,
        ...size,
      });
    const stage = boards.get(element.id);
    const target = stage?.targets[element.id];
    const pinned =
      !!stage &&
      !!point &&
      (stage.pinned.has(element.id) ||
        (moved.has(element.id) &&
          !!target &&
          Math.hypot(point.x - target.x, point.y - target.y) > 2));
    if (!pinned) {
      const kept = previous?.revision === BOARD_PIN ? undefined : previous;
      return contractPlacement(kept ?? { element: element.id, x: 0, y: 0, ...size });
    }
    return contractPlacement({
      element: element.id,
      x: point.x - stage.lane.corner.x,
      y: point.y - stage.lane.corner.y,
      ...size,
      revision: BOARD_PIN,
    });
  });
}
const relationLabels: Record<Exclude<CanvasLink["kind"], "thread">, () => string> = {
  dependency: m.work_env_relation_depends_on,
  reference: m.work_env_relation_uses,
  supports: m.work_env_relation_supports,
  uses: m.work_env_relation_uses,
  depends_on: m.work_env_relation_depends_on,
  same_as: m.work_env_relation_same_as,
  contradicts: m.work_env_relation_contradicts,
};
/** Relations between the person's own cards; a lane already says how its parts belong. */
export function environmentLinks(snapshot: WorkEnvironmentSnapshot): CanvasLink[] {
  const pictures = subjectPictures(snapshot);
  const ids = new Set(
    snapshot.elements.flatMap((element) =>
      pictures.has(element.id) ||
      element.reference.kind === "source" ||
      laneElement(snapshot, element)
        ? []
        : [element.id],
    ),
  );
  return (snapshot.relations ?? []).flatMap((relation) =>
    ids.has(relation.from) && ids.has(relation.to)
      ? [
          {
            id: `relation:${relation.id}`,
            source: relation.from,
            target: relation.to,
            kind: relation.kind,
            label: relationLabels[relation.kind](),
          },
        ]
      : [],
  );
}

/** What the mark says beside the orb: a word, a host or a file, while it lasts. */
function agentCaption(
  execution: WorkExecutionFact,
  doing: AgentDoing,
  signal: string | undefined,
): string | undefined {
  if (signal === "waiting_for_human") return m.work_line_waiting_for_you();
  const running = (execution.steps ?? []).filter((step) => step.status === "running");
  switch (doing) {
    case "thinking":
      return m.work_line_thinking();
    case "searching":
      return m.work_line_searching();
    case "reading": {
      const read = running.find((step) => step.kind.kind === "read");
      const where = read?.kind.kind === "read" ? host(read.kind.url).replace(/^www\./u, "") : "";
      return where ? m.work_line_reading({ host: where }) : m.work_line_reading_web();
    }
    case "working": {
      for (const step of running) {
        const kind = step.kind;
        if (kind.kind === "read_file")
          return m.work_line_reading_file({ name: fileName(kind.path) });
        if (kind.kind === "search_files") return m.work_line_searching_files();
        if (kind.kind === "write_file" || kind.kind === "edit_file")
          return m.work_line_writing_file({ name: fileName(kind.path) });
      }
      return m.work_env_working();
    }
    case "writing":
      return m.work_line_writing();
    case "done":
      return undefined;
  }
}

/**
 * Where the mark stands for what the agent does: by the request while it
 * thinks, by the trail while it searches or works in files, off the page it
 * reads, at the board's corner while it writes into it.
 */
function standFor(
  execution: WorkExecutionFact,
  doing: AgentDoing,
  stage: WorkStage,
): CanvasPosition {
  const rect = (id: string | undefined): Rect | undefined =>
    id ? stage.lane.rects[id] : undefined;
  const trail = rect(stage.column.trail);
  switch (doing) {
    case "reading": {
      const read = (execution.steps ?? []).find(
        (step) =>
          step.status === "running" && (step.kind.kind === "read" || step.kind.kind === "discover"),
      );
      const page = pageGroups(execution, []).find((group) =>
        group.steps.some((step) => step.id === read?.id),
      )?.id;
      const card = rect(stage.column.pages.find((id) => id === page) ?? stage.column.pages[0]);
      return standBeside(card ?? trail ?? stage.place);
    }
    case "searching":
    case "working":
      return standBeside(trail ?? stage.place);
    case "writing":
    case "done": {
      const board = stage.lane.board;
      return { x: board.x - 12, y: board.y - 12 };
    }
    default:
      return standBeside(stage.place);
  }
}

/** Transient agent presence for objectives with live executions; never persisted. */
export function environmentAgents(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  activity: (objective: string) => string | undefined,
  /** Every lane of the canvas; the mark stands in the one its run serves. */
  stages: readonly WorkStage[] = [],
): { items: CanvasItem[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const positions: Record<string, CanvasPosition> = {};
  for (const element of snapshot.elements) {
    if (element.reference.kind !== "objective") continue;
    const projection = objectives.get(element.reference.objective);
    const execution = projection?.executions.at(-1);
    if (!projection || !execution || !isLive(projection, execution)) continue;
    const signal = activity(projection.work.id);
    const label = signal ? activityLabel(signal) : undefined;
    let seed = 0;
    for (const char of projection.work.id) seed = (seed * 31 + char.charCodeAt(0)) % 9973;
    const id = `agent:${element.id}`;
    const doing = agentDoing(execution);
    const caption = agentCaption(execution, doing, signal);
    const status =
      caption ??
      label ??
      (execution.status === "cancel_requested"
        ? m.work_activity_cancelling()
        : execution.status === "approved"
          ? m.work_env_agent_idle()
          : m.work_env_working());
    const item: CanvasItem = {
      id,
      type: "agent",
      kind: m.work_env_agent(),
      title: m.work_env_agent(),
      detail: "",
      status,
      agent: {
        seed,
        activity: signal ?? "",
        objective: projection.work.id,
        doing,
        ...(caption ? { caption } : {}),
      },
    };
    const stage = stages.find((stage) => stage.executions.includes(execution.id));
    if (stage) {
      const stand = standFor(execution, doing, stage);
      positions[id] = stand;
      item.agent!.stand = stand;
      // The trail already says what it is doing; the orb only shows where.
      if (stage.trail.some((line) => line.live)) delete item.agent!.caption;
    }
    items.push(item);
  }
  return { items, positions };
}

/**
 * One Sources card per request, at the foot of its process column: the pages
 * its searches cited and the files its steps opened, once each; the pages that
 * would not open; and once the run is done, the frames of the pages it read.
 */
export function environmentSources(
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  stages: readonly WorkStage[],
  pages: (objective: string) => readonly WorkPageV1[] = () => [],
): CanvasItem[] {
  const items: CanvasItem[] = [];
  for (const stage of stages) {
    const card = stage.column.sources;
    if (!card) continue;
    const runs = stageRuns(stage, objectives).filter(isAgentExecution);
    const rows: SourceRow[] = [];
    const seen = new Set<string>();
    const read = new Set<string>();
    const refusals = new Map<string, string>();
    for (const execution of runs) {
      for (const row of sourceRows(execution))
        if (!seen.has(row.key)) {
          seen.add(row.key);
          rows.push(row);
        }
      for (const step of execution.steps ?? []) {
        if (step.kind.kind !== "read") continue;
        if (step.status === "succeeded") read.add(step.kind.url);
        else if (step.status === "failed" && step.note?.trim())
          refusals.set(step.kind.url, step.note.trim());
      }
    }
    const unread = runs.flatMap((execution) => unreadPages(execution));
    const recorded = pages(stage.objective);
    const frames = stage.live
      ? []
      : runs
          .flatMap((execution) => pageGroups(execution, recorded))
          .flatMap((group) => {
            const frame = group.page?.frame
              ? pageFrameUrl(group.page.attempt, group.page.step, group.page.frame.generation)
              : null;
            return frame
              ? [
                  {
                    key: group.id,
                    url: clipText(group.url, DETAIL_TEXT),
                    host: host(group.url),
                    frame,
                  },
                ]
              : [];
          })
          .slice(0, FRAMES);
    items.push({
      id: card,
      type: "sources",
      kind: m.work_env_sources(),
      title: !rows.length
        ? m.work_env_sources()
        : rows.length === 1
          ? m.work_env_source_one()
          : m.work_env_sources_count({ count: rows.length }),
      detail: "",
      status: "",
      size: { width: stage.lane.rects[card]!.width, height: stage.lane.rects[card]!.height },
      ...(stage.live ? { active: true } : {}),
      sources: rows.slice(0, SOURCE_ROWS).map((row) => {
        const refusal = row.url && !read.has(row.url) ? refusals.get(row.url) : undefined;
        return refusal ? { ...row, note: clipText(refusal, ROW_TEXT) } : row;
      }),
      ...(unread.length ? { unread } : {}),
      ...(frames.length ? { frames } : {}),
    });
  }
  return items;
}

/**
 * The pages a live run reads, in its process column: the one it is on and the
 * last it read. A page held for a person says so; the rest say how they went.
 */
export function environmentPages(
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  stages: readonly WorkStage[],
  pages: (objective: string) => readonly WorkPageV1[],
  /** The run's current activity, so a page held for a hidden window says so. */
  activity: (objective: string) => string | undefined = () => undefined,
  /** The pages this work is holding open for a person, if any are. */
  human: (objective: string) => readonly WorkHumanPageV1[] = () => [],
): CanvasItem[] {
  const items: CanvasItem[] = [];
  for (const stage of stages) {
    if (!stage.column.pages.length) continue;
    const projection = objectives.get(stage.objective);
    if (!projection) continue;
    const recorded = pages(projection.work.id);
    const waiting = human(projection.work.id);
    const paused = activity(projection.work.id) === "paused";
    for (const execution of stageRuns(stage, objectives)) {
      const running = isLive(projection, execution);
      const opened = recorded.filter((page) => page.execution === execution.id);
      for (const entry of pageGroups(execution, recorded)) {
        if (!stage.column.pages.includes(entry.id)) continue;
        const { id, url } = entry;
        const pageHost = host(url);
        const live = running && entry.steps.some((step) => step.status === "running");
        const succeeded = entry.steps.some((step) => step.status === "succeeded");
        const refused = !succeeded && !live ? (entry.steps.at(-1)?.note?.trim() ?? "") : "";
        const frame = entry.page?.frame
          ? pageFrameUrl(entry.page.attempt, entry.page.step, entry.page.frame.generation)
          : null;
        // Rust names a held page by its read step; one card folds several of them.
        const held = heldPage(
          waiting.filter((candidate) =>
            entry.steps.some(
              (step) =>
                step.id === candidate.id.step &&
                !opened.some(
                  (record) =>
                    record.step === candidate.id.step && record.attempt !== candidate.id.attempt,
                ),
            ),
          ),
        );
        const observed = entry.steps.findLast((step) => step.local?.page_title?.trim())?.local
          ?.page_title;
        const shown = !!entry.tab && !entry.steps.length;
        const account = entry.steps.find((step) => step.account?.badge)?.account?.host;
        const rect = stage.lane.rects[id]!;
        items.push({
          id,
          type: "page",
          kind: m.work_env_page(),
          title: clipText(
            observed?.trim() || entry.tab?.title.trim() || pageHost || m.work_env_page(),
            TITLE_TEXT,
          ),
          detail: clipText(url, DETAIL_TEXT),
          status: shown
            ? m.work_env_tab_caption()
            : held
              ? (phaseLabel(held.phase) ?? m.work_line_waiting_for_you())
              : live
                ? paused
                  ? m.work_line_paused()
                  : m.work_env_page_live()
                : succeeded
                  ? m.work_env_page_read()
                  : clipText(refused, STATUS_TEXT) || m.work_env_status_failed(),
          size: { width: rect.width, height: rect.height },
          page: {
            url,
            host: pageHost,
            frame,
            live,
            ...(held ? { human: humanPage(held) } : {}),
            ...(account ? { account } : {}),
            ...(shown ? { tab: true } : {}),
          },
          ...(shown || live || succeeded || held ? {} : { unavailable: true }),
        });
      }
    }
  }
  return items;
}

/** Each lane's board: its head, its blocks, and its trail in the process column. */
export function environmentBoards(
  stages: readonly WorkStage[],
  open: string | null,
  /** The one result a block stands for, when it stands for one: its note and tasks come from it. */
  artifactOf: (id: string) => CanvasItem["artifact"] = () => undefined,
): CanvasItem[] {
  const items: CanvasItem[] = [];
  for (const stage of stages) {
    const size = (id: string) => {
      const rect = stage.lane.rects[id]!;
      return { width: rect.width, height: rect.height };
    };
    if (stage.column.trail)
      items.push({
        id: stage.column.trail,
        type: "trail",
        kind: m.work_board_trail(),
        title: stage.request,
        detail: "",
        status: "",
        size: size(stage.column.trail),
        trail: stage.trail,
        ...(stage.live ? { active: true } : {}),
      });
    if (stage.column.head)
      items.push({
        id: stage.column.head,
        type: "head",
        kind: "",
        title: clipText(stage.board.title, TITLE_TEXT),
        detail: clipText(stage.board.lead, DETAIL_TEXT),
        status: "",
        size: size(stage.column.head),
        ...(stage.board.more
          ? {
              block: {
                data: stage.board.more,
                sources: stage.board.sources,
                open: false,
                live: stage.live,
              },
            }
          : {}),
      });
    for (const block of stage.board.blocks)
      items.push({
        id: block.id,
        type: "block",
        kind: block.kind,
        title: clipText(block.title ?? "", TITLE_TEXT),
        detail: "",
        status: "",
        size: size(block.id),
        ...(artifactOf(block.id) ? { artifact: artifactOf(block.id) } : {}),
        block: {
          data: block,
          sources: stage.board.sources,
          open: block.id === open,
          live: stage.live,
        },
      });
  }
  return items;
}

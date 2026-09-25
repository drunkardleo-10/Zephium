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
import { firstRequest, threadOf, type WorkStage } from "./project-environment-thread";
import {
  commandCards,
  displayPath,
  fileCards,
  host,
  LANE_PLACEMENT,
  laneFacts,
  laneElement,
  observedTitle,
  pageGroups,
  sourceRows,
} from "./project-environment-stage";
import {
  LANE,
  SIZES,
  laneShape,
  markStand,
  placeLane,
  type LaneGroup,
  type MarkStand,
} from "./stage-layout";
import { fileName } from "./work-files";
import { heldPage, humanPage, phaseLabel } from "./work-human";
import { resultPlan, stepIcon, stepId } from "./plan-steps";
import { DIAGRAM, diagramKindLabel, diagramLayout, diagramNodeId } from "./diagram";
import { pageFrameUrl } from "$domain/resources";
import {
  clipText,
  type CanvasCluster,
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
    if (element.reference.kind === "subject" || element.reference.kind === "finding") {
      const reference = element.reference;
      const execution = projection?.executions.find(
        (execution) => execution.id === reference.execution,
      );
      const artifact = execution?.artifacts.find((artifact) => artifact.id === reference.artifact);
      const view = artifact && execution ? artifactView(artifact, execution) : undefined;
      if (reference.kind === "subject") {
        const subject =
          view &&
          (view.content.kind === "matrix" ||
            view.content.kind === "findings" ||
            view.content.kind === "sources")
            ? view.content.subjects[reference.index]
            : undefined;
        const facts = subject && execution ? subjectFacts(execution, subject) : [];
        return {
          id: element.id,
          type: "subject",
          area: element.area,
          kind: m.work_env_subject(),
          title: clipText(subject?.name ?? m.work_artifact_unavailable(), TITLE_TEXT),
          detail: clipText(subject?.descriptor ?? "", DETAIL_TEXT),
          status: area,
          subject,
          ...(facts.length ? { facts } : {}),
          unavailable: !subject,
        };
      }
      const finding =
        view && view.content.kind === "findings" ? view.content.items[reference.index] : undefined;
      return {
        id: element.id,
        type: "finding",
        area: element.area,
        kind: m.work_env_finding(),
        title: clipText(finding?.claim ?? m.work_artifact_unavailable(), TITLE_TEXT),
        detail: clipText(finding?.detail ?? "", DETAIL_TEXT),
        status: area,
        finding,
        unavailable: !finding,
      };
    }
    if (element.reference.kind === "artifact") {
      const reference = element.reference;
      const execution = projection?.executions.find(
        (execution) => execution.id === reference.execution,
      );
      const artifact = execution?.artifacts.find((artifact) => artifact.id === reference.artifact);
      const view = artifact && execution ? artifactView(artifact, execution) : undefined;
      // A findings artifact is one card listing its claims; the lift reads the artifact.
      if (view?.content.kind === "findings") {
        const { items, subjects } = view.content;
        return {
          id: element.id,
          type: "findings",
          area: element.area,
          kind: m.work_env_findings(),
          title: clipText(artifact?.title ?? m.work_env_findings(), TITLE_TEXT),
          detail: "",
          status: view.reviewLabel,
          artifact: view,
          findings: {
            items: items.map((item) => {
              const subject = item.subject === undefined ? undefined : subjects[item.subject];
              return {
                claim: clipText(item.claim, TITLE_TEXT),
                confidence: item.confidence,
                ...(subject ? { subject: clipText(subject.name, ROW_TEXT) } : {}),
                evidence: item.evidence.length,
              };
            }),
            total: items.length,
          },
        };
      }
      return {
        id: element.id,
        type: "result",
        area: element.area,
        kind: m.work_env_result(),
        title: clipText(artifact?.title ?? m.work_artifact_unavailable(), TITLE_TEXT),
        detail: "",
        status: view?.reviewLabel ?? m.work_env_open_to_load(),
        artifact: view,
        layout: "artifact",
      };
    }
    // The person's sentence, carried plainly: state belongs to the agent line.
    const lane = projection ? threadOf(element.id, projection)[0] : undefined;
    return {
      id: element.id,
      type: "objective",
      area: element.area,
      kind: m.work_env_request(),
      title: clipText(projection ? firstRequest(projection) : m.work_env_request(), TITLE_TEXT),
      detail: "",
      status: area,
      ...(projection && lane
        ? laneFacts(
            projection.executions.filter((execution) => lane.executions.includes(execution.id)),
          )
        : {}),
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
 * lane (see `environmentStages`), so only the person's own elements bring an
 * absolute position; a lane card brings its size once it is saved in lane terms.
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
      snapshot.view.placements
        .filter((place) => !lane.has(place.element) || place.revision === LANE_PLACEMENT)
        .map((place) => [place.element, { width: place.width, height: place.height }]),
    ),
    positions: Object.fromEntries(own.map((place) => [place.element, { x: place.x, y: place.y }])),
    viewport: { x: snapshot.view.x, y: snapshot.view.y, zoom: snapshot.view.zoom_milli / 1000 },
  };
}

/**
 * The placements a view saves: the person's own elements where they stand, a
 * lane card as its offset from its lane place. A lane card the canvas has not
 * placed yet keeps what it had.
 */
export function viewPlacements(
  snapshot: WorkEnvironmentSnapshot,
  view: CanvasView,
  stages: readonly WorkStage[],
): WorkElementPlacement[] {
  const bases = new Map<string, CanvasPosition>();
  for (const stage of stages) {
    bases.set(stage.card, stage.place);
    for (const [id, position] of Object.entries(stage.layout.positions)) bases.set(id, position);
  }
  return snapshot.elements.map((element) => {
    const previous = snapshot.view.placements.find((place) => place.element === element.id);
    const point = view.positions[element.id];
    const size = {
      width: view.sizes?.[element.id]?.width ?? previous?.width ?? 280,
      height: view.sizes?.[element.id]?.height ?? previous?.height ?? 160,
    };
    if (!laneElement(snapshot, element))
      return {
        element: element.id,
        x: Math.round(point?.x ?? previous?.x ?? 0),
        y: Math.round(point?.y ?? previous?.y ?? 0),
        ...size,
      };
    const base = bases.get(element.id);
    if (!base || !point) return previous ?? { element: element.id, x: 0, y: 0, ...size };
    return {
      element: element.id,
      x: Math.round(point.x - base.x),
      y: Math.round(point.y - base.y),
      ...size,
      revision: LANE_PLACEMENT,
    };
  });
}
const relationLabels: Record<
  Exclude<CanvasLink["kind"], "path" | "thread" | "diagram">,
  () => string
> = {
  dependency: m.work_env_relation_depends_on,
  reference: m.work_env_relation_uses,
  supports: m.work_env_relation_supports,
  uses: m.work_env_relation_uses,
  depends_on: m.work_env_relation_depends_on,
  same_as: m.work_env_relation_same_as,
  contradicts: m.work_env_relation_contradicts,
};
/** Relations between cards; a request is never an end, the stage's path already reads from it. */
export function environmentLinks(snapshot: WorkEnvironmentSnapshot): CanvasLink[] {
  const pictures = subjectPictures(snapshot);
  const ids = new Set(
    snapshot.elements.flatMap((element) =>
      pictures.has(element.id) ||
      element.reference.kind === "source" ||
      element.reference.kind === "objective"
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

/** Where the mark stands for what the agent does: the page it reads, the result it finished. */
function standFor(execution: WorkExecutionFact, doing: AgentDoing, stage: WorkStage): MarkStand {
  if (doing === "reading") {
    const read = (execution.steps ?? []).find(
      (step) =>
        step.status === "running" && (step.kind.kind === "read" || step.kind.kind === "discover"),
    );
    const page = pageGroups(execution, []).find((group) =>
      group.steps.some((step) => step.id === read?.id),
    )?.id;
    return page ? { doing, page } : { doing };
  }
  if (doing === "done") {
    const result = stage.contents.results?.members[0]?.id;
    return result ? { doing, result } : { doing };
  }
  return { doing };
}

/**
 * Transient agent presence for objectives with live executions; never
 * persisted. The mark stands by what its running steps act on: the request
 * while it thinks, Worked with while it searches, the page it reads, the
 * local row, the group it writes into, and the result once it is done.
 */
export function environmentAgents(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  activity: (objective: string) => string | undefined,
  /** Every lane of the canvas; the mark stands in the one its run serves. */
  stages: readonly WorkStage[] = [],
): { items: CanvasItem[]; links: CanvasLink[]; positions: Record<string, CanvasPosition> } {
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
      const sizes = Object.fromEntries(
        Object.values(stage.contents).flatMap((group) =>
          (group?.members ?? []).map((member) => [member.id, member.size] as const),
        ),
      );
      const stand = markStand(stage.layout, standFor(execution, doing, stage), sizes, stage.slots);
      positions[id] = stand;
      item.agent!.stand = stand;
    }
    items.push(item);
  }
  return { items, links: [], positions };
}

/**
 * One Sources card per execution: the pages its searches cited and the files
 * its steps opened, counted and listed together. Individual sources are rows,
 * never cards; the lane's group edges already say what came from them.
 */
export function environmentSources(
  _snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  /** The message each run served; its Sources card stands in that lane. */
  stages: readonly WorkStage[],
): { items: CanvasItem[]; links: CanvasLink[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const positions: Record<string, CanvasPosition> = {};
  for (const stage of stages) {
    const projection = objectives.get(stage.objective);
    if (!projection) continue;
    for (const id of stage.executions) {
      const execution = projection.executions.find((entry) => entry.id === id);
      if (!execution || !isAgentExecution(execution)) continue;
      const running = isLive(projection, execution);
      const rows = sourceRows(execution);
      if (!rows.length) continue;
      // A page that would not be read says why, on the row that cites it.
      const read = new Set<string>();
      const refusals = new Map<string, string>();
      for (const step of execution.steps ?? []) {
        if (step.kind.kind !== "read") continue;
        if (step.status === "succeeded") read.add(step.kind.url);
        else if (step.status === "failed" && step.note?.trim())
          refusals.set(step.kind.url, step.note.trim());
      }
      const card = `sources:${stage.element}:${execution.id}`;
      items.push({
        id: card,
        type: "sources",
        kind: m.work_env_sources(),
        title:
          rows.length === 1
            ? m.work_env_source_one()
            : m.work_env_sources_count({ count: rows.length }),
        detail: "",
        status: "",
        ...(running ? { active: true } : {}),
        sources: rows.slice(0, SOURCE_ROWS).map((row) => {
          const refusal = row.url && !read.has(row.url) ? refusals.get(row.url) : undefined;
          return refusal ? { ...row, note: clipText(refusal, ROW_TEXT) } : row;
        }),
      });
      const position = stage.layout.positions[card];
      if (position) positions[card] = position;
    }
  }
  return { items, links: [], positions };
}

/** Pages the agent opened, a grid per stage: every run of the stage keeps the
 * last frame it recorded, and a live page still shows the live one. Past the
 * cluster's cap the rest only count on its label. */
export function environmentPages(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  /** Every message of the thread; a run's pages stand in its stage's cluster. */
  stages: readonly WorkStage[],
  pages: (objective: string) => readonly WorkPageV1[],
  /** The run's current activity, so a page held for a hidden window says so. */
  activity: (objective: string) => string | undefined = () => undefined,
  /** Unused since the agent ties itself to nothing; kept for the caller's shape. */
  _present: ReadonlySet<string> = new Set(),
  /** The pages this work is holding open for a person, if any are. */
  human: (objective: string) => readonly WorkHumanPageV1[] = () => [],
): { items: CanvasItem[]; links: CanvasLink[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
  const positions: Record<string, CanvasPosition> = {};
  for (const stage of stages) {
    const projection = objectives.get(stage.objective);
    if (!projection) continue;
    const recorded = pages(projection.work.id);
    const waiting = human(projection.work.id);
    const paused = activity(projection.work.id) === "paused";
    const runs = stage.executions.flatMap((id) => {
      const execution = projection.executions.find((entry) => entry.id === id);
      return execution && isAgentExecution(execution)
        ? [{ execution, groups: pageGroups(execution, recorded) }]
        : [];
    });
    // The frames this call sees decide the group; the lane may have counted fewer.
    const seen = runs.flatMap(({ groups }) => groups.map((group) => group.id));
    const counted = stage.contents.pages?.members.map((member) => member.id) ?? [];
    const layout =
      JSON.stringify(seen) === JSON.stringify(counted)
        ? stage.layout
        : placeLane(
            stage.place,
            laneShape({
              ...stage.contents,
              pages: { members: seen.map((id) => ({ id, size: SIZES.page })) },
            }),
            stage.slots,
          );
    for (const { execution, groups } of runs) {
      // Only the run that is still going marks its pages live; an earlier
      // stage keeps its last frames and says nothing about now.
      const running = isLive(projection, execution);
      const opened = recorded.filter((page) => page.execution === execution.id);
      const hubs = new Map<string, string>();
      // What each Found card cites, so a page can say it is evidence for it.
      const cites: { id: string; extractions: Set<string> }[] = [];
      for (const candidate of snapshot.elements) {
        const reference = candidate.reference;
        if (!("execution" in reference) || reference.execution !== execution.id) continue;
        const artifact = execution.artifacts.find((artifact) => artifact.id === reference.artifact);
        if (!artifact) continue;
        if (reference.kind === "subject") {
          const subject = subjectsOf(artifact)[reference.index];
          if (subject) hubs.set(subjectKey(subject), candidate.id);
        } else if (reference.kind === "finding" && artifact.data.kind === "findings") {
          const evidence = artifact.data.items[reference.index]?.evidence ?? [];
          cites.push({
            id: candidate.id,
            extractions: new Set(
              evidence.flatMap((index) => artifact.evidence[index]?.extraction_id ?? []),
            ),
          });
        } else if (reference.kind === "artifact" && artifact.data.kind === "findings")
          cites.push({
            id: candidate.id,
            extractions: new Set(artifact.evidence.map((link) => link.extraction_id)),
          });
      }
      for (const entry of groups) {
        const position = layout.positions[entry.id];
        if (!position) continue;
        const { id, url } = entry;
        const pageHost = host(url);
        const live = running && entry.steps.some((step) => step.status === "running");
        const succeeded = entry.steps.some((step) => step.status === "succeeded");
        // Rust says why a read gave up or was cut off; the card says it instead of "Failed".
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
        // The title a read observed names the card; the host stands in until one did.
        const observed = entry.steps.findLast((step) => step.local?.page_title?.trim())?.local
          ?.page_title;
        items.push({
          id,
          type: "page",
          kind: m.work_env_page(),
          title: clipText(observed?.trim() || pageHost || m.work_env_page(), TITLE_TEXT),
          detail: clipText(url, DETAIL_TEXT),
          status: held
            ? (phaseLabel(held.phase) ?? m.work_line_waiting_for_you())
            : live
              ? paused
                ? m.work_line_paused()
                : m.work_env_page_live()
              : succeeded
                ? m.work_env_page_read()
                : clipText(refused, STATUS_TEXT) || m.work_env_status_failed(),
          page: { url, host: pageHost, frame, live, ...(held ? { human: humanPage(held) } : {}) },
          ...(live || succeeded || held ? {} : { unavailable: true }),
        });
        positions[id] = position;
        // A page is evidence for the subjects its reads established and the findings citing them.
        const linked = new Set<string>();
        const tie = (target: string) => {
          if (linked.has(target)) return;
          linked.add(target);
          links.push({
            id: `page-evidence:${id}:${target}`,
            source: id,
            target,
            kind: "supports",
            role: "evidence",
          });
        };
        const produced = new Set<string>();
        for (const step of entry.steps) {
          if (step.evidence) produced.add(step.evidence);
          for (const artifactId of step.artifacts ?? []) {
            produced.add(artifactId);
            const artifact = execution.artifacts.find((artifact) => artifact.id === artifactId);
            for (const subject of artifact ? subjectsOf(artifact) : []) {
              const hub = hubs.get(subjectKey(subject));
              if (hub) tie(hub);
            }
          }
        }
        for (const cited of cites)
          if ([...produced].some((extraction) => cited.extractions.has(extraction))) tie(cited.id);
      }
    }
  }
  return { items, links, positions };
}

/** The files and commands each run touched: the stage's Work cluster. */
export function environmentFiles(
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  stages: readonly WorkStage[],
): { items: CanvasItem[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const positions: Record<string, CanvasPosition> = {};
  for (const stage of stages) {
    const projection = objectives.get(stage.objective);
    if (!projection) continue;
    const start = items.length;
    for (const id of stage.executions) {
      const execution = projection.executions.find((entry) => entry.id === id);
      if (!execution || !isAgentExecution(execution)) continue;
      for (const card of fileCards(execution))
        items.push({
          id: card.id,
          type: "file",
          kind: m.work_env_file(),
          title: card.file.name,
          detail: displayPath(card.path),
          status: "",
          file: card.file,
        });
      for (const card of commandCards(execution))
        items.push({
          id: card.id,
          type: "command",
          kind: m.work_env_command(),
          title: card.command.line,
          detail: "",
          status: "",
          command: card.command,
        });
    }
    for (const item of items.slice(start)) {
      const position = stage.layout.positions[item.id];
      if (position) positions[item.id] = position;
    }
  }
  return { items, positions };
}

/**
 * A result's plan, one card per step beside it in Made. A step ties, only
 * while focused, to the subjects it names.
 */
export function environmentSteps(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  stages: readonly WorkStage[],
): { items: CanvasItem[]; links: CanvasLink[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
  const positions: Record<string, CanvasPosition> = {};
  for (const stage of stages) {
    const projection = objectives.get(stage.objective);
    if (!projection) continue;
    const named = snapshot.elements.flatMap((element) => {
      const reference = element.reference;
      if (reference.kind !== "subject" || !stage.executions.includes(reference.execution))
        return [];
      const execution = projection.executions.find((entry) => entry.id === reference.execution);
      const artifact = execution?.artifacts.find((entry) => entry.id === reference.artifact);
      const name = artifact ? subjectsOf(artifact)[reference.index]?.name.trim() : undefined;
      return name && name.length >= 3 ? [{ id: element.id, name: name.toLowerCase() }] : [];
    });
    for (const element of snapshot.elements) {
      const reference = element.reference;
      if (reference.kind !== "artifact" || !stage.executions.includes(reference.execution))
        continue;
      const execution = projection.executions.find((entry) => entry.id === reference.execution);
      const artifact = execution?.artifacts.find((entry) => entry.id === reference.artifact);
      if (!execution || !artifact) continue;
      resultPlan(artifactView(artifact, execution).content).forEach((step, index) => {
        const id = stepId(element.id, index);
        const text = clipText(step.text, TITLE_TEXT);
        const detail = clipText(step.detail ?? "", DETAIL_TEXT);
        const icon = stepIcon(text) === "check" && detail ? stepIcon(detail) : stepIcon(text);
        items.push({
          id,
          type: "step",
          kind: m.work_env_step(),
          title: text,
          detail,
          status: "",
          step: { index: index + 1, text, icon },
        });
        const position = stage.layout.positions[id];
        if (position) positions[id] = position;
        const said = `${text} ${detail}`.toLowerCase();
        for (const subject of named)
          if (said.includes(subject.name))
            links.push({
              id: `step-subject:${id}:${subject.id}`,
              source: id,
              target: subject.id,
              kind: "uses",
              role: "named",
              label: m.work_env_relation_uses(),
            });
      });
    }
  }
  return { items, links, positions };
}

/**
 * A diagram's parts, one small card each in an area beside the diagram's
 * cover, joined at rest by what flows between them. The area is captioned by
 * the diagram's title and each layer by its name.
 */
export function environmentDiagrams(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  stages: readonly WorkStage[],
): {
  items: CanvasItem[];
  links: CanvasLink[];
  positions: Record<string, CanvasPosition>;
  clusters: CanvasCluster[];
} {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
  const positions: Record<string, CanvasPosition> = {};
  const clusters: CanvasCluster[] = [];
  for (const stage of stages) {
    const projection = objectives.get(stage.objective);
    if (!projection) continue;
    for (const element of snapshot.elements) {
      const reference = element.reference;
      if (reference.kind !== "artifact" || !stage.executions.includes(reference.execution))
        continue;
      const execution = projection.executions.find((entry) => entry.id === reference.execution);
      const artifact = execution?.artifacts.find((entry) => entry.id === reference.artifact);
      if (!execution || !artifact) continue;
      const view = artifactView(artifact, execution);
      if (view.content.kind !== "diagram") continue;
      const content = view.content;
      const id = (node: string) => diagramNodeId(element.id, node);
      for (const node of content.nodes) {
        const card = id(node.id);
        items.push({
          id: card,
          type: "diagram",
          kind: diagramKindLabel(node.kind),
          title: clipText(node.name, TITLE_TEXT),
          detail: clipText(node.note ?? "", DETAIL_TEXT),
          status: "",
          diagram: {
            kind: node.kind,
            ...(node.vendor ? { vendor: node.vendor } : {}),
            ...(node.note ? { note: node.note } : {}),
            ...(node.layer ? { layer: node.layer } : {}),
          },
        });
        const position = stage.layout.positions[card];
        if (position) positions[card] = position;
      }
      const { at, layers, plates, width, height } = diagramLayout(content);
      content.edges.forEach((edge, index) => {
        const plate = plates[index];
        links.push({
          id: `diagram-edge:${element.id}:${index}`,
          source: id(edge.from),
          target: id(edge.to),
          kind: "diagram",
          ...(at[edge.from]?.x === at[edge.to]?.x ? { down: true } : {}),
          ...(edge.label ? { label: clipText(edge.label, ROW_TEXT) } : {}),
          ...(edge.label && plate ? { plate } : {}),
        });
      });
      const band = layers.length ? DIAGRAM.layer : 0;
      clusters.push({
        id: diagramArea(stage.card, element.id),
        label:
          content.nodes.length === 1
            ? m.work_card_diagram_part_one()
            : m.work_card_diagram_parts({ count: content.nodes.length }),
        title: clipText(artifact.title, TITLE_TEXT),
        opens: element.id,
        ...(view.knowledge ? { knowledge: true } : {}),
        more: 0,
        members: content.nodes.map((node) => id(node.id)),
        inset: LANE.inset,
        live: stage.live,
        tone: "area",
        extent: { width, height: height - band },
        ...(layers.length
          ? {
              caption: LANE.caption + DIAGRAM.layer,
              layers: layers.map((layer) => ({
                name: layer.name,
                members: layer.nodes.map(id),
              })),
            }
          : {}),
      });
    }
  }
  return { items, links, positions, clusters };
}
const diagramArea = (card: string, result: string) => `group:${card}:diagram:${result}`;

/** Each bare result's title and whether it is drawn from what the agent knows. */
export function resultHeads(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  stages: readonly WorkStage[],
): Map<string, { title: string; knowledge: boolean }> {
  const bare = new Set(
    stages.flatMap(
      (stage) =>
        stage.contents.results?.members.flatMap((member) => (member.bare ? [member.id] : [])) ?? [],
    ),
  );
  const heads = new Map<string, { title: string; knowledge: boolean }>();
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (reference.kind !== "artifact" || !bare.has(element.id)) continue;
    const execution = objectives
      .get(reference.objective)
      ?.executions.find((entry) => entry.id === reference.execution);
    const artifact = execution?.artifacts.find((entry) => entry.id === reference.artifact);
    if (!execution || !artifact) continue;
    const view = artifactView(artifact, execution);
    heads.set(element.id, { title: clipText(view.title, TITLE_TEXT), knowledge: !!view.knowledge });
  }
  return heads;
}

/** A group's caption counts what it holds: "2 pages · 3 local steps", "4 subjects", "6 steps". */
function caption(group: LaneGroup): string {
  const { pages = 0, work = 0, subjects = 0, findings = 0, results = 0 } = group.counts;
  const parts =
    group.kind === "worked"
      ? [
          pages === 1
            ? m.work_env_cluster_page_one()
            : pages
              ? m.work_env_cluster_pages({ count: pages })
              : "",
          work === 1
            ? m.work_env_cluster_work_one()
            : work
              ? m.work_env_cluster_work({ count: work })
              : "",
        ]
      : group.kind === "found"
        ? [
            subjects === 1
              ? m.work_env_cluster_subject_one()
              : subjects
                ? m.work_env_cluster_subjects({ count: subjects })
                : "",
            findings === 1
              ? m.work_env_findings()
              : findings
                ? m.work_env_cluster_findings({ count: findings })
                : "",
          ]
        : [results === 1 ? m.work_env_result() : m.work_env_cluster_results({ count: results })];
  return parts.filter(Boolean).join(" · ") || m.work_env_sources();
}
const stepsCaption = (count: number) =>
  count === 1 ? m.work_env_cluster_step_one() : m.work_env_cluster_steps({ count });

/**
 * Each lane's groups and the edges that rest between them: request → Worked
 * with → Found → Made, skipping what the lane lacks, and each result into its
 * steps. Edges end at a group's box, never at a card inside it.
 */
export function environmentClusters(
  stages: readonly WorkStage[],
  /** A bare result's title and knowledge: its steps caption carries them. */
  heads: ReadonlyMap<string, { title: string; knowledge: boolean }> = new Map(),
): {
  clusters: CanvasCluster[];
  links: CanvasLink[];
} {
  const clusters: CanvasCluster[] = [];
  const links: CanvasLink[] = [];
  for (const stage of stages) {
    let from = stage.card;
    for (const group of stage.layout.groups) {
      const id = `group:${stage.card}:${group.kind}`;
      const inner = (group.steps ?? []).map((entry) => ({
        ...entry,
        id: `group:${stage.card}:steps:${entry.result}`,
      }));
      const areas = group.diagrams ?? [];
      const steps = new Set([
        ...inner.flatMap((entry) => entry.members),
        ...areas.flatMap((entry) => entry.members),
      ]);
      const bare = new Set(
        stage.contents.results?.members.flatMap((member) => (member.bare ? [member.id] : [])),
      );
      for (const entry of inner) {
        const head = bare.has(entry.result) ? heads.get(entry.result) : undefined;
        clusters.push({
          id: entry.id,
          label: stepsCaption(entry.members.length),
          more: 0,
          members: entry.members,
          inset: LANE.inset,
          live: stage.live,
          ...(head
            ? {
                title: head.title,
                opens: entry.result,
                ...(head.knowledge ? { knowledge: true } : {}),
              }
            : {}),
        });
      }
      clusters.push({
        id,
        label: caption(group),
        more: group.more,
        members: group.members.filter((member) => !steps.has(member)),
        ...(inner.length || areas.length
          ? {
              within: [
                ...inner.map((entry) => entry.id),
                ...areas.map((entry) => diagramArea(stage.card, entry.result)),
              ],
            }
          : {}),
        inset: LANE.pad,
        live: stage.live,
      });
      links.push({
        id: `path:${stage.card}:${group.kind}`,
        source: from,
        target: id,
        kind: "path",
      });
      for (const entry of inner.filter((entry) => !bare.has(entry.result)))
        links.push({
          id: `path:${stage.card}:steps:${entry.result}`,
          source: entry.result,
          target: entry.id,
          kind: "path",
        });
      from = id;
    }
  }
  return { clusters, links };
}

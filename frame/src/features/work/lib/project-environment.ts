import type {
  WorkFileEvidenceV1,
  WorkEnvironmentSnapshot,
  TabView,
  ResourceSummary,
  WorkHumanPageV1,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { activityLabel, artifactView } from "./project-work";
import { agentDoing, agentLine, isAgentExecution, isLive, type AgentDoing } from "./agent-steps";
import { subjectFacts, subjectKey, subjectsOf } from "./subjects";
import { firstRequest, type WorkStage } from "./project-environment-thread";
import {
  commandCards,
  displayPath,
  fileCards,
  host,
  observedTitle,
  pageGroups,
  sourceRows,
} from "./project-environment-stage";
import { SIZES, stageLayout, stageStand, type ClusterKind } from "./stage-layout";
import { heldPage, humanPage, phaseLabel } from "./work-human";
import { resultPlan, stepIcon, stepId } from "./plan-steps";
import { pageFrameUrl } from "$domain/resources";
import {
  clipText,
  defaultSize,
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
  notes: readonly ResourceSummary[],
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
  notes: readonly ResourceSummary[],
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
        favicon: tab?.favicon ?? null,
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
        detail: note?.updated_at ?? "",
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
export function environmentView(snapshot: WorkEnvironmentSnapshot): CanvasView {
  return {
    areas: Object.fromEntries(
      (snapshot.view.areas ?? []).map((place) => [
        place.area,
        { x: place.x, y: place.y, width: place.width, height: place.height },
      ]),
    ),
    sizes: Object.fromEntries(
      snapshot.view.placements.map((place) => [
        place.element,
        { width: place.width, height: place.height },
      ]),
    ),
    positions: Object.fromEntries(
      snapshot.view.placements.map((place) => [place.element, { x: place.x, y: place.y }]),
    ),
    viewport: { x: snapshot.view.x, y: snapshot.view.y, zoom: snapshot.view.zoom_milli / 1000 },
  };
}
const relationLabels: Record<Exclude<CanvasLink["kind"], "path" | "thread">, () => string> = {
  dependency: m.work_env_relation_depends_on,
  reference: m.work_env_relation_uses,
  supports: m.work_env_relation_supports,
  uses: m.work_env_relation_uses,
  depends_on: m.work_env_relation_depends_on,
  same_as: m.work_env_relation_same_as,
  contradicts: m.work_env_relation_contradicts,
  working: m.work_env_relation_working,
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

/** Where the agent stands while it does each thing: the first cluster the stage has. */
const STANDS: Record<AgentDoing, readonly ClusterKind[]> = {
  thinking: [],
  searching: ["sources"],
  reading: ["pages"],
  working: ["work"],
  writing: ["subjects", "findings", "results"],
  done: ["results", "findings", "subjects", "work", "pages", "sources"],
};

/**
 * Transient agent presence for objectives with live executions; never
 * persisted. The agent stands beside what its running steps act on: the
 * request while it thinks, Sources while it searches, the Pages cluster while
 * it reads, the Work cluster for files and commands, the objects it writes,
 * and the result once it is done.
 */
export function environmentAgents(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  activity: (objective: string) => string | undefined,
  /** The message the live run serves; the agent waits beside that request. */
  stages: readonly WorkStage[] = [],
): { items: CanvasItem[]; links: CanvasLink[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
  const positions: Record<string, CanvasPosition> = {};
  const pictures = subjectPictures(snapshot);
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
    const line = agentLine(execution);
    const doing = agentDoing(execution);
    const stage = stages.find((stage) => stage.executions.includes(execution.id));
    const request =
      stage?.place ?? snapshot.view.placements.find((place) => place.element === element.id);
    const item: CanvasItem = {
      id,
      type: "agent",
      kind: m.work_env_agent(),
      title: m.work_env_agent(),
      detail: "",
      status:
        label ??
        (execution.status === "cancel_requested"
          ? m.work_activity_cancelling()
          : execution.status === "approved"
            ? m.work_env_agent_idle()
            : m.work_env_working()),
      agent: { seed, activity: signal ?? "", objective: projection.work.id, doing },
    };
    if (line) item.agent!.line = line;
    if (request) {
      const stand = stageStand(stage?.layout ?? { clusters: [], positions: {}, extent: 0 }, {
        request,
        at: STANDS[doing],
        size: defaultSize(item),
        avoid: stages
          .filter((other) => other !== stage)
          .flatMap((other) => [
            other.place,
            ...other.layout.clusters.map((cluster) => cluster.box),
          ]),
      });
      positions[id] = stand;
      item.agent!.stand = stand;
    }
    items.push(item);
    // A cited source and a subject's picture are rows and pictures, not cards:
    // the agent ties itself to what it just published, never to one of those.
    const steps = execution.steps ?? [];
    const lastPublish = steps.findLast((step) => step.kind.kind === "publish");
    const latestTurn = Math.max(0, ...steps.map((step) => step.turn));
    if (!lastPublish || lastPublish.turn !== latestTurn) continue;
    const published = lastPublish.artifacts ?? [];
    for (const target of snapshot.elements)
      if (
        "artifact" in target.reference &&
        target.reference.kind !== "source" &&
        !pictures.has(target.id) &&
        target.reference.execution === execution.id &&
        published.includes(target.reference.artifact)
      )
        links.push({ id: `working:${target.id}`, source: id, target: target.id, kind: "working" });
  }
  return { items, links, positions };
}

/**
 * One Sources card per execution: the pages its searches cited and the files
 * its steps opened, counted and listed together. Individual sources are rows,
 * never cards, and later objects connect to the stage that established them.
 */
export function environmentSources(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  /** The message each run served; its Sources card stands in that stage. */
  stages: readonly WorkStage[],
): { items: CanvasItem[]; links: CanvasLink[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
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
      const seen = new Set(rows.map((row) => row.key));
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
      // Findings and published objects tie to the stage whose pages they cite.
      for (const candidate of snapshot.elements) {
        const reference = candidate.reference;
        if (
          (reference.kind !== "finding" && reference.kind !== "artifact") ||
          reference.execution !== execution.id
        )
          continue;
        const artifact = execution.artifacts.find((entry) => entry.id === reference.artifact);
        if (!artifact) continue;
        const cited =
          reference.kind === "artifact"
            ? artifact.evidence
            : (artifact.data.kind === "findings"
                ? (artifact.data.items[reference.index]?.evidence ?? [])
                : []
              ).flatMap((position) =>
                artifact.evidence[position] ? [artifact.evidence[position]!] : [],
              );
        if (!cited.some((link) => seen.has(`${link.extraction_id}:${link.source_id}`))) continue;
        links.push({
          id: `sources-support:${card}:${candidate.id}`,
          source: card,
          target: candidate.id,
          kind: "supports",
          label: m.work_env_relation_supports(),
        });
      }
    }
  }
  return { items, links, positions };
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
  /** The agent presences the scene already has; a tie to an absent one is no tie. */
  present: ReadonlySet<string> = new Set(),
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
    const agent = `agent:${stage.element}`;
    const runs = stage.executions.flatMap((id) => {
      const execution = projection.executions.find((entry) => entry.id === id);
      return execution && isAgentExecution(execution)
        ? [{ execution, groups: pageGroups(execution, recorded) }]
        : [];
    });
    // The frames this call sees decide the cluster; the stage may have counted fewer.
    const layout = stageLayout(stage.place, {
      ...stage.contents,
      pages: {
        members: runs.flatMap(({ groups }) =>
          groups.map((group) => ({ id: group.id, size: SIZES.page })),
        ),
      },
    });
    for (const { execution, groups } of runs) {
      // Only the run that is still going marks its pages live; an earlier
      // stage keeps its last frames and says nothing about now.
      const running = isLive(projection, execution);
      const opened = recorded.filter((page) => page.execution === execution.id);
      const hubs = new Map<string, string>();
      for (const candidate of snapshot.elements) {
        const reference = candidate.reference;
        if (reference.kind !== "subject" || reference.execution !== execution.id) continue;
        const artifact = execution.artifacts.find((artifact) => artifact.id === reference.artifact);
        const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
        if (subject) hubs.set(subjectKey(subject), candidate.id);
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
        items.push({
          id,
          type: "page",
          kind: m.work_env_page(),
          title: pageHost || m.work_env_page(),
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
        if (live && present.has(agent))
          links.push({ id: `working:${id}`, source: agent, target: id, kind: "working" });
        const linked = new Set<string>();
        for (const step of entry.steps)
          for (const artifactId of step.artifacts ?? []) {
            const artifact = execution.artifacts.find((artifact) => artifact.id === artifactId);
            for (const subject of artifact ? subjectsOf(artifact) : []) {
              const hub = hubs.get(subjectKey(subject));
              if (!hub || linked.has(hub)) continue;
              linked.add(hub);
              links.push({
                id: `page-subject:${id}:${hub}`,
                source: id,
                target: hub,
                kind: "supports",
              });
            }
          }
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
 * A result's plan, one card per step in the stage's Plan cluster. A step ties
 * (only while focused) to the subjects it names and to the Sources it rests on.
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
    const sources = new Set(stage.contents.sources?.members.map((member) => member.id) ?? []);
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
      const source = `sources:${stage.element}:${execution.id}`;
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
              label: m.work_env_relation_uses(),
            });
        if (sources.has(source))
          links.push({
            id: `step-sources:${id}`,
            source,
            target: id,
            kind: "supports",
            label: m.work_env_relation_supports(),
          });
      });
    }
  }
  return { items, links, positions };
}

const CLUSTER_LABELS: Record<ClusterKind, (count: number) => string> = {
  sources: (count) => m.work_env_sources_count({ count }),
  pages: (count) =>
    count === 1 ? m.work_env_cluster_page_one() : m.work_env_cluster_pages({ count }),
  work: (count) =>
    count === 1 ? m.work_env_cluster_work_one() : m.work_env_cluster_work({ count }),
  subjects: (count) =>
    count === 1 ? m.work_env_cluster_subject_one() : m.work_env_cluster_subjects({ count }),
  findings: (count) => m.work_env_cluster_findings({ count }),
  results: (count) => m.work_env_cluster_results({ count }),
  plan: (count) =>
    count === 1 ? m.work_env_cluster_step_one() : m.work_env_cluster_steps({ count }),
};
/** Kinds that always gather under a label; the rest do only when they hold several cards. */
const LABELLED = new Set<ClusterKind>(["pages", "work", "subjects", "plan"]);

/**
 * Each stage's clusters and the path that reads through them: request, then
 * every non-empty cluster in order. A path edge attaches to a cluster's node,
 * or to the card itself when a cluster is a single card.
 */
export function environmentClusters(stages: readonly WorkStage[]): {
  clusters: CanvasCluster[];
  links: CanvasLink[];
} {
  const clusters: CanvasCluster[] = [];
  const links: CanvasLink[] = [];
  for (const stage of stages) {
    let from = { kind: "request", id: stage.card };
    for (const cluster of stage.layout.clusters) {
      const grouped = LABELLED.has(cluster.kind) || cluster.members.length > 1;
      const id = grouped ? `cluster:${stage.card}:${cluster.kind}` : cluster.members[0]!;
      if (grouped)
        clusters.push({
          id,
          label: CLUSTER_LABELS[cluster.kind](cluster.members.length + cluster.more),
          more: cluster.more,
          members: cluster.members,
        });
      links.push({
        id: `path:${stage.card}:${from.kind}:${cluster.kind}`,
        source: from.id,
        target: id,
        kind: "path",
      });
      from = { kind: cluster.kind, id };
    }
  }
  return { clusters, links };
}

import type {
  WorkArtifactV1,
  WorkFileEvidenceV1,
  WorkFileRecordV1,
  WorkEnvironmentSnapshot,
  TabView,
  ResourceSummary,
  WorkExecutionFact,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { artifactView } from "./project-work";
import { agentLine, isAgentExecution } from "./agent-steps";
import { subjectFacts, subjectKey, subjectsOf } from "./subjects";
import { COLUMNS, SOURCES_SIZE } from "./organize";
import { fileFolder } from "./work-files";
import { pageFrameUrl } from "$domain/resources";

/** How many cited pages one Sources card lists; the lift shows the rest. */
const SOURCE_ROWS = 24;
/** Steps that work inside a granted folder; each settles onto a file record. */
const FILE_STEPS = ["list", "read_file", "search_files", "write_file", "edit_file"];

function host(url: string | undefined): string {
  if (!url) return "";
  try {
    return new URL(url).host;
  } catch {
    return "";
  }
}
type SourceRow = {
  key: string;
  url: string;
  where: string;
  title: string;
  file?: { record: string; path: string; kind: string };
};
/** A file a step disclosed: the folder it sits in stands where a host would. */
function fileRow(key: string, record: WorkFileRecordV1, title?: string): SourceRow {
  return {
    key,
    url: "",
    where: fileFolder(record.file.path),
    title: title || record.file.name,
    file: { record: record.id, path: record.file.path, kind: record.file.kind },
  };
}
/** The pages and files behind one evidence collection, in the order it cites them. */
function collectionRows(execution: WorkExecutionFact, artifact: WorkArtifactV1): SourceRow[] {
  if (artifact.data.kind !== "evidence_collection") return [];
  return (artifact.data.entries ?? []).flatMap((entry) => {
    const link = artifact.evidence[entry.evidence];
    if (!link) return [];
    const key = `${link.extraction_id}:${link.source_id}`;
    const search = execution.provider_evidence?.find(
      (candidate) => candidate.id === link.extraction_id,
    );
    const citation = search?.evidence.citations[link.source_id - 1];
    const url = cleanUrl(citation?.url);
    if (url)
      return [{ key, url, where: host(url), title: entry.title || citation?.title || host(url) }];
    // A granted folder is not a place the pane can open: the row names the file.
    const record = execution.file_evidence?.find(
      (candidate) => candidate.id === link.extraction_id,
    );
    return record ? [fileRow(key, record, entry.title)] : [];
  });
}
/** The provider's tracking parameter is not part of the page the card opens. */
function cleanUrl(url: string | undefined): string | undefined {
  if (!url) return undefined;
  try {
    const parsed = new URL(url);
    parsed.searchParams.delete("utm_source");
    return parsed.toString();
  } catch {
    return url;
  }
}
import type { CanvasItem, CanvasLink, CanvasPosition, CanvasView } from "./canvas-model";
import type { MediaAssetV1 } from "$domain/resources";
import * as m from "$shared/i18n/messages";

/**
 * Media elements a subject admitted its picture into: Rust records the origin
 * as a `uses` relation from the subject to the media element. The picture
 * belongs to that subject card and never becomes a card of its own.
 */
function subjectPictures(snapshot: WorkEnvironmentSnapshot): Set<string> {
  const subjects = new Set<string>();
  const resources = new Set<string>();
  for (const element of snapshot.elements) {
    if (element.reference.kind === "subject") subjects.add(element.id);
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
        title: link.title || host(link.url) || link.url,
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
          title: subject?.name ?? m.work_artifact_unavailable(),
          detail: subject?.descriptor ?? "",
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
        title: finding?.claim ?? m.work_artifact_unavailable(),
        detail: finding?.detail ?? "",
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
      return {
        id: element.id,
        type: "result",
        area: element.area,
        kind: m.work_env_result(),
        title: artifact?.title ?? m.work_artifact_unavailable(),
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
      title: projection?.work.objective.slice(0, 512) ?? m.work_env_request(),
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
const relationLabels: Record<CanvasLink["kind"], () => string> = {
  dependency: m.work_env_relation_depends_on,
  reference: m.work_env_relation_uses,
  supports: m.work_env_relation_supports,
  uses: m.work_env_relation_uses,
  depends_on: m.work_env_relation_depends_on,
  same_as: m.work_env_relation_same_as,
  contradicts: m.work_env_relation_contradicts,
  working: m.work_env_relation_working,
};
export function environmentLinks(snapshot: WorkEnvironmentSnapshot): CanvasLink[] {
  const pictures = subjectPictures(snapshot);
  const ids = new Set(
    snapshot.elements.flatMap((element) =>
      pictures.has(element.id) || element.reference.kind === "source" ? [] : [element.id],
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

const agentLabels: Record<string, () => string> = {
  planning: m.work_activity_planning,
  delegating: m.work_activity_delegating,
  searching: m.work_activity_searching,
  reading: m.work_activity_reading,
  interacting: m.work_activity_interacting,
  verifying: m.work_activity_verifying,
  recovering: m.work_activity_recovering,
  comparing: m.work_activity_comparing,
  producing_artifact: m.work_activity_producing,
  paused: m.work_activity_paused,
  waiting_for_approval: m.work_activity_approval,
  waiting_for_human: m.work_activity_human,
  cancelling: m.work_activity_cancelling,
  finishing: m.work_activity_finishing,
};
/** Transient agent presence for objectives with live executions; never persisted.
 * The primary avatar stands beside what it just placed; a worker avatar appears
 * beside it while a native step browses. */
export function environmentAgents(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  activity: (objective: string) => string | undefined,
): { items: CanvasItem[]; links: CanvasLink[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
  const positions: Record<string, CanvasPosition> = {};
  for (const element of snapshot.elements) {
    if (element.reference.kind !== "objective") continue;
    const projection = objectives.get(element.reference.objective);
    const execution = projection?.executions.at(-1);
    if (
      !projection ||
      !execution ||
      !["running", "cancel_requested", "approved"].includes(execution.status) ||
      projection.interrupted.includes(execution.id)
    )
      continue;
    const signal = activity(projection.work.id);
    const label = signal ? agentLabels[signal]?.() : undefined;
    let seed = 0;
    for (const char of projection.work.id) seed = (seed * 31 + char.charCodeAt(0)) % 9973;
    const id = `agent:${element.id}`;
    const line = agentLine(execution);
    items.push({
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
      agent: {
        seed,
        activity: signal ?? "",
        objective: projection.work.id,
        ...(line ? { line } : {}),
      },
    });
    const anchor = snapshot.view.placements.find((place) => place.element === element.id);
    const placement = (target: string) =>
      snapshot.view.placements.find((place) => place.element === target);
    const home = anchor ? { x: anchor.x + anchor.width + 48, y: anchor.y } : undefined;
    const steps = execution.steps ?? [];
    const elementsOf = (artifacts: readonly string[]) =>
      snapshot.elements.filter(
        (candidate) =>
          "artifact" in candidate.reference &&
          candidate.reference.execution === execution.id &&
          artifacts.includes(candidate.reference.artifact),
      );
    // The agent stands beside what it acts on now: the objects it just placed,
    // or the request while it thinks, searches and reads.
    const working = new Set<string>();
    const lastPublish = [...steps].reverse().find((step) => step.kind.kind === "publish");
    const latestTurn = Math.max(0, ...steps.map((step) => step.turn));
    let stand = home;
    if (lastPublish && lastPublish.turn === latestTurn) {
      const placed = elementsOf(lastPublish.artifacts ?? []);
      for (const target of placed) working.add(target.id);
      const newest = placed
        .map((target) => placement(target.id))
        .filter((place): place is NonNullable<typeof place> => !!place)
        .sort((a, b) => b.y - a.y)[0];
      if (newest) stand = { x: newest.x + newest.width + 40, y: newest.y };
    }
    if (stand) positions[id] = stand;
    for (const target of working)
      links.push({ id: `working:${target}`, source: id, target, kind: "working" });
    for (const step of steps) {
      if (step.status !== "running" || step.kind.kind !== "discover") continue;
      const worker = `${id}:${step.id}`;
      items.push({
        id: worker,
        type: "agent",
        kind: m.work_env_worker(),
        title: m.work_env_worker(),
        detail: "",
        status: m.work_env_browsing(),
        agent: { seed: seed + 1, activity: "reading", objective: projection.work.id, worker: true },
      });
      links.push({ id: `agent-worker:${worker}`, source: id, target: worker, kind: "working" });
      if (stand) positions[worker] = { x: stand.x, y: stand.y + 140 };
    }
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
): {
  items: CanvasItem[];
  links: CanvasLink[];
  positions: Record<string, CanvasPosition>;
  /** The group each cited page belongs to, so a page card joins the same stage. */
  groups: Map<string, string>;
} {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
  const positions: Record<string, CanvasPosition> = {};
  const groups = new Map<string, string>();
  for (const element of snapshot.elements) {
    if (element.reference.kind !== "objective") continue;
    const projection = objectives.get(element.reference.objective);
    if (!projection) continue;
    const anchor = snapshot.view.placements.find((place) => place.element === element.id);
    const home = anchor ? { x: anchor.x, y: anchor.y + anchor.height + 48 } : { x: 80, y: 320 };
    let index = 0;
    for (const execution of projection.executions) {
      if (!isAgentExecution(execution)) continue;
      const running =
        ["running", "cancel_requested", "approved"].includes(execution.status) &&
        !projection.interrupted.includes(execution.id);
      const seen = new Set<string>();
      const files = new Set<string>();
      const rows: SourceRow[] = [];
      const admit = (row: SourceRow) => {
        if (seen.has(row.key) || (row.file && files.has(row.file.record))) return;
        seen.add(row.key);
        if (row.file) files.add(row.file.record);
        rows.push(row);
      };
      for (const step of execution.steps ?? []) {
        if (step.kind.kind === "search") {
          for (const id of step.artifacts ?? []) {
            const artifact = execution.artifacts.find((entry) => entry.id === id);
            if (artifact?.data.kind !== "evidence_collection") continue;
            for (const row of collectionRows(execution, artifact)) admit(row);
          }
          continue;
        }
        if (!FILE_STEPS.includes(step.kind.kind) || !step.evidence) continue;
        const record = execution.file_evidence?.find((candidate) => candidate.id === step.evidence);
        if (record) admit(fileRow(`file:${record.id}`, record));
      }
      if (!rows.length) continue;
      // A continuation served an earlier request; the stage keeps saying which.
      const request = execution.spec.request?.trim() ?? "";
      const served = request && request !== projection.work.objective.trim() ? request : "";
      const id = `sources:${element.id}:${execution.id}`;
      items.push({
        id,
        type: "sources",
        kind: m.work_env_sources(),
        title:
          rows.length === 1
            ? m.work_env_source_one()
            : m.work_env_sources_count({ count: rows.length }),
        detail: served.slice(0, 240),
        status: "",
        ...(running ? { active: true } : {}),
        sources: rows.slice(0, SOURCE_ROWS),
      });
      positions[id] = { x: home.x, y: home.y + index * (SOURCES_SIZE.height + 24) };
      index += 1;
      links.push({
        id: `sources-of:${id}`,
        source: element.id,
        target: id,
        kind: "uses",
        label: m.work_env_relation_uses(),
      });
      for (const row of rows) if (row.url) groups.set(row.url, id);
      // Findings and published objects hang off the stage whose pages they cite.
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
          id: `sources-support:${id}:${candidate.id}`,
          source: id,
          target: candidate.id,
          kind: "supports",
          label: m.work_env_relation_supports(),
        });
      }
    }
  }
  return { items, links, positions, groups };
}

/** Pages the agent opened: transient cards beside the goal that show the newest
 * frame while a step works there and keep the last one after it settles. */
export function environmentPages(
  snapshot: WorkEnvironmentSnapshot,
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  pages: (objective: string) => readonly WorkPageV1[],
  /** The Sources card each cited page came from, so a page joins its stage. */
  groups: ReadonlyMap<string, string> = new Map(),
  /** The run's current activity, so a page held for a hidden window says so. */
  activity: (objective: string) => string | undefined = () => undefined,
): { items: CanvasItem[]; links: CanvasLink[]; positions: Record<string, CanvasPosition> } {
  const items: CanvasItem[] = [];
  const links: CanvasLink[] = [];
  const positions: Record<string, CanvasPosition> = {};
  for (const element of snapshot.elements) {
    if (element.reference.kind !== "objective") continue;
    const projection = objectives.get(element.reference.objective);
    const execution = projection?.executions.at(-1);
    if (!projection || !execution || !isAgentExecution(execution)) continue;
    const opened = pages(projection.work.id).filter((page) => page.execution === execution.id);
    const paused = activity(projection.work.id) === "paused";
    // One card per page: every step that opened the same URL folds into it.
    const byUrl = new Map<
      string,
      { steps: typeof execution.steps & object; page?: WorkPageV1; running: boolean }
    >();
    for (const step of execution.steps ?? []) {
      if (step.kind.kind !== "read" && step.kind.kind !== "discover") continue;
      const page = opened.find((page) => page.step === step.id);
      const url = page?.url || (step.kind.kind === "read" ? step.kind.url : "");
      if (!url) continue;
      const entry = byUrl.get(url) ?? { steps: [], running: false };
      entry.steps.push(step);
      if (page?.frame && (!entry.page?.frame || page.live)) entry.page = page;
      else entry.page ??= page;
      entry.running ||= step.status === "running";
      byUrl.set(url, entry);
    }
    if (!byUrl.size) continue;
    const anchor = snapshot.view.placements.find((place) => place.element === element.id);
    const home = anchor
      ? { x: COLUMNS.pages(anchor.x), y: anchor.y + anchor.height + 48 }
      : { x: COLUMNS.pages(80), y: 320 };
    const hubs = new Map<string, string>();
    for (const candidate of snapshot.elements) {
      const reference = candidate.reference;
      if (reference.kind !== "subject" || reference.execution !== execution.id) continue;
      const artifact = execution.artifacts.find((artifact) => artifact.id === reference.artifact);
      const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
      if (subject) hubs.set(subjectKey(subject), candidate.id);
    }
    const agent = `agent:${element.id}`;
    let index = 0;
    for (const [url, entry] of byUrl) {
      const first = entry.steps[0]!;
      const pageHost = host(url);
      const live = entry.running;
      const succeeded = entry.steps.some((step) => step.status === "succeeded");
      const frame = entry.page?.frame
        ? pageFrameUrl(entry.page.attempt, entry.page.step, entry.page.frame.generation)
        : null;
      const id = `page:${element.id}:${first.id}`;
      items.push({
        id,
        type: "page",
        kind: m.work_env_page(),
        title: pageHost || m.work_env_page(),
        detail: url,
        status: live
          ? paused
            ? m.work_line_paused()
            : m.work_env_page_live()
          : succeeded
            ? m.work_env_page_read()
            : m.work_env_status_failed(),
        page: { url, host: pageHost, frame, live },
      });
      positions[id] = { x: home.x, y: home.y + index * 260 };
      index += 1;
      const group = groups.get(url);
      if (group)
        links.push({
          id: `sources-page:${group}:${id}`,
          source: group,
          target: id,
          kind: "uses",
          label: m.work_env_relation_uses(),
        });
      if (live) links.push({ id: `working:${id}`, source: agent, target: id, kind: "working" });
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
  return { items, links, positions };
}

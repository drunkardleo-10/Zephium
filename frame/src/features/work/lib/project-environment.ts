import type {
  WorkEnvironmentReference,
  WorkEnvironmentSnapshot,
  TabView,
  ResourceSummary,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { artifactView } from "./project-work";
import { agentLine, isAgentExecution } from "./agent-steps";
import { subjectFacts } from "./subjects";

function host(url: string | undefined): string {
  if (!url) return "";
  try {
    return new URL(url).host;
  } catch {
    return "";
  }
}
/** The cited URL and title behind one source entry, from the provider record. */
function sourceCitation(
  projection: WorkRuntimeProjection | undefined,
  reference: Extract<WorkEnvironmentReference, { kind: "source" }>,
) {
  const execution = projection?.executions.find((entry) => entry.id === reference.execution);
  const artifact = execution?.artifacts.find((entry) => entry.id === reference.artifact);
  if (!execution || artifact?.data.kind !== "evidence_collection") return undefined;
  const entry = artifact.data.entries?.[reference.index];
  const link = entry ? artifact.evidence[entry.evidence] : undefined;
  const record = link
    ? execution.provider_evidence?.find((record) => record.id === link.extraction_id)
    : undefined;
  const citation = link ? record?.evidence.citations[link.source_id - 1] : undefined;
  return entry ? { entry, url: cleanUrl(citation?.url), title: citation?.title } : undefined;
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

function objectiveStatus(projection: WorkRuntimeProjection): string {
  const execution = projection.executions.at(-1);
  if (execution) {
    if (projection.interrupted.includes(execution.id)) return m.work_interrupted();
    const agent = isAgentExecution(execution);
    const labels = {
      approved: m.work_env_status_approved,
      running: m.work_env_status_running,
      cancel_requested: m.work_env_status_stopping,
      completed: agent ? m.work_env_status_done : m.work_env_status_completed,
      needs_review: agent ? m.work_env_status_done : m.work_review_required,
      cancelled: m.work_env_status_cancelled,
      failed: m.work_env_status_failed,
      interrupted: m.work_interrupted,
    };
    return labels[execution.status]();
  }
  return projection.work.status === "draft"
    ? m.work_env_status_draft()
    : projection.work.status === "needs_input"
      ? m.work_env_status_needs_input()
      : m.work_env_status_plan_ready();
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
  return snapshot.elements.map((element) => {
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
    const projection = objectives.get(element.reference.objective);
    if (element.reference.kind === "source") {
      const citation = sourceCitation(projection, element.reference);
      return {
        id: element.id,
        type: "source",
        area: element.area,
        kind: m.work_env_source(),
        title: citation?.entry.title ?? m.work_artifact_unavailable(),
        detail: host(citation?.url),
        status: area,
        ...(citation?.url ? { source: { url: citation.url, role: citation.entry.role } } : {}),
        unavailable: !citation,
      };
    }
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
    return {
      id: element.id,
      type: "objective",
      area: element.area,
      kind: m.work_env_objective(),
      title: projection?.work.objective.slice(0, 512) ?? m.work_env_objective(),
      detail: projection
        ? m.work_env_objective_status({
            status: objectiveStatus(projection),
          })
        : m.work_env_open_to_load(),
      status: area,
    };
  });
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
  const ids = new Set(snapshot.elements.map((element) => element.id));
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
  waiting_for_approval: m.work_activity_approval,
  waiting_for_human: m.work_activity_human,
  cancelling: m.work_activity_cancelling,
  finishing: m.work_activity_finishing,
};
/** Transient agent presence for objectives with live executions; never persisted.
 * The primary avatar links to the sources of its latest turn; a worker avatar
 * appears beside the source a native step is reading. */
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
    const beside = (target: string, dy = 0) => {
      const place = placement(target);
      return place ? { x: place.x + place.width + 40, y: place.y + dy } : undefined;
    };
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
    // the sources it is reading, or the goal while it thinks or searches.
    const working = new Set<string>();
    const lastPublish = [...steps].reverse().find((step) => step.kind.kind === "publish");
    const latestTurn = Math.max(0, ...steps.map((step) => step.turn));
    const reading = steps.find(
      (step) =>
        step.status === "running" && (step.kind.kind === "read" || step.kind.kind === "discover"),
    );
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
    const searches = steps.filter(
      (step) => step.kind.kind === "search" && (step.artifacts?.length ?? 0) > 0,
    );
    const latestSearchTurn = Math.max(0, ...searches.map((step) => step.turn));
    const recent = new Set(
      searches
        .filter((step) => step.turn === latestSearchTurn)
        .flatMap((step) => step.artifacts ?? []),
    );
    const sources = snapshot.elements.filter(
      (candidate) =>
        candidate.reference.kind === "source" && candidate.reference.execution === execution.id,
    );
    let newestSource: { x: number; y: number; width: number } | undefined;
    for (const source of sources) {
      if (source.reference.kind !== "source" || !recent.has(source.reference.artifact)) continue;
      if (latestTurn === latestSearchTurn) working.add(source.id);
      const place = placement(source.id);
      if (place && (!newestSource || place.y > newestSource.y)) newestSource = place;
    }
    if (newestSource && !(lastPublish && lastPublish.turn === latestTurn))
      stand = { x: newestSource.x + newestSource.width + 40, y: newestSource.y };
    if (reading?.kind.kind === "read") {
      const target = sources.find((source) => {
        const citation =
          source.reference.kind === "source"
            ? sourceCitation(projection, source.reference)
            : undefined;
        return citation?.url === reading.kind.url;
      });
      if (target) {
        working.add(target.id);
        stand = beside(target.id, -8) ?? stand;
      }
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

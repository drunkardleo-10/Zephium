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
import { agentDoing, isLive, type AgentDoing } from "./agent-steps";
import { subjectFacts, subjectKey, subjectsOf } from "./subjects";
import { firstRequest, type WorkStage } from "./project-environment-thread";
import { host, observedTitle } from "./project-environment-stage";
import { BOARD_PIN, laneElement, partShape } from "./project-environment-board";
import type { RunPart } from "./run/parts";
import { hostOf, siteKey, siteName } from "./run/site";
import type { SourceRow } from "./project-environment-stage";
import { RUN } from "./run/layout";
import { PART, partLead } from "./run/part-size";
import { fileName } from "./work-files";
import { linkVideo } from "./link-media";
import { heldPage, humanPage, phaseLabel } from "./work-human";
import { mediaUrl, pageFrameUrl } from "$domain/resources";
import {
  clipText,
  type PartNeed,
  type PartPage,
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
const STATUS_TEXT = 256;
import type { MediaAssetV1 } from "$domain/resources";
import * as m from "$shared/i18n/messages";

/**
 * Media elements a subject, a link or an object admitted its pictures into: Rust records
 * the origin as a `uses` relation from that element to the media element. The
 * picture belongs to its card and never becomes a card of its own.
 */
function subjectPictures(snapshot: WorkEnvironmentSnapshot): Set<string> {
  const subjects = new Set<string>();
  const resources = new Set<string>();
  for (const element of snapshot.elements) {
    if (
      element.reference.kind === "subject" ||
      element.reference.kind === "link" ||
      element.reference.kind === "artifact"
    )
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
    // A video's admitted thumbnail is its poster, read from the profile rather than the web.
    const src =
      image && item.object?.view.kind === "media" ? mediaUrl(image.profile, image.digest) : null;
    if (src && item.object?.view.kind === "media")
      item = {
        ...item,
        object: { ...item.object, view: { ...item.object.view, poster: { src } } },
      };
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
      // A tab the work held that has since closed says nothing: it drops out, never a blank card.
      if (!tab) return [];
      let origin = "";
      try {
        if (tab.url) {
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
        title: tab.title || host(tab.url ?? undefined) || m.work_env_browser_resource(),
        detail: origin,
        status: area,
        icon: tab.icon ?? null,
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
      const title = clipText(
        observedTitle(objectives, link.url) || link.title || host(link.url) || link.url,
        TITLE_TEXT,
      );
      // A video is its poster, played in place; the poster is the admitted thumbnail.
      const video = linkVideo(element.id, link.url, link.title ? { title: link.title } : {});
      if (video)
        return {
          id: element.id,
          type: "object" as const,
          area: element.area,
          kind: m.work_env_link(),
          title,
          detail: origin,
          status: area,
          object: { view: video, live: false },
        };
      return {
        id: element.id,
        type: "link" as const,
        area: element.area,
        kind: m.work_env_link(),
        // A read's observed page title names the card; the stored title stays as it is.
        title,
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
  for (const stage of stages) {
    for (const block of stage.board.blocks) boards.set(block.id, stage);
    for (const object of stage.objects) boards.set(object.id, stage);
  }
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
const relationLabels: Record<Exclude<CanvasLink["kind"], "thread" | "flow">, () => string> = {
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
 * Where the orb stands for what the agent does: at the start of the run's
 * lines while it thinks, on the live page of the part it works in, at the
 * result's anchor while it writes.
 */
function standFor(doing: AgentDoing, stage: WorkStage): CanvasPosition {
  const spine = stage.place.y + RUN.spine - MARK / 2;
  const working = stage.parts.find((part) => part.state === "running");
  const rect = working ? stage.lane.rects[working.id] : undefined;
  if (rect && doing !== "writing" && doing !== "done") {
    if (working!.helper === "browser" && working!.pages.length)
      return {
        x: rect.x + partLead + PART.window - MARK / 2,
        y: rect.y - MARK / 2,
      };
    // Beside a row that shows its own work, off its end on the row's line.
    return { x: rect.x + rect.width + 8, y: rect.y + RUN.labelMid - MARK / 2 };
  }
  if (doing === "writing" || doing === "done")
    return { x: stage.lane.corner.x - RUN.air - MARK, y: spine };
  return { x: RUN.request + RUN.air, y: spine };
}
const MARK = 24;

/** The orb's colour for a work, the same wherever it shows. */
function agentSeed(work: string): number {
  let seed = 0;
  for (const char of work) seed = (seed * 31 + char.charCodeAt(0)) % 9973;
  return seed;
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
    const seed = agentSeed(projection.work.id);
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
      const stand = standFor(doing, stage);
      positions[id] = stand;
      item.agent!.stand = stand;
    }
    items.push(item);
  }
  return { items, positions };
}

/** What a search part drew on, one row per site: its mark and the name it goes by. */
function citedSites(rows: readonly SourceRow[]) {
  const sites = new Map<string, { key: string; url: string; where: string; title: string }>();
  for (const row of rows) {
    const host = hostOf(row.url);
    const key = siteKey(host);
    if (!host || sites.has(key)) continue;
    const titles = rows.flatMap((other) =>
      siteKey(hostOf(other.url)) === key ? [other.title] : [],
    );
    sites.set(key, { key: row.key, url: row.url, where: host, title: siteName(host, titles) });
  }
  return [...sites.values()];
}

/** Command line tools by the service a person knows them as. */
const TOOLS: Record<string, string> = { gh: "GitHub", glab: "GitLab" };

/** A part's need as its row says it, named the way a person names the thing. */
function needView(need: NonNullable<RunPart["need"]>): PartNeed {
  switch (need.kind) {
    case "sign_in":
    case "allow_site":
      return { kind: need.kind, target: siteName(need.host), address: `https://${need.host}` };
    case "allow_folder":
      return {
        kind: need.kind,
        target: need.path.replace(/\/+$/u, "").split("/").at(-1) || need.path,
        address: need.path,
      };
    case "use_connection":
      return { kind: need.kind, target: TOOLS[need.connection] ?? need.connection };
    case "retry":
      return { kind: need.kind, target: need.host ? siteName(need.host) : "" };
  }
}

/** Found things a part names in its summary, by the kind of thing. */
function partSummary(part: RunPart, stage: WorkStage): string | undefined {
  if (part.names?.length) return part.names.join(", ");
  if (part.summary && part.state !== "running") return part.summary;
  if (part.state === "failed") return m.work_part_failed();
  if (part.state === "running")
    return part.helper === "research"
      ? m.work_line_searching()
      : part.helper === "browser"
        ? m.work_part_reading()
        : m.work_env_working();
  let things = 0;
  for (const block of stage.board.blocks) {
    if (stage.found.get(block.id) !== part.id) continue;
    things += block.kind === "gallery" ? block.entities.length : 1;
  }
  if (things) return things === 1 ? m.work_part_found_one() : m.work_part_found({ count: things });
  return undefined;
}

/**
 * Each part of every run: its name and mark, and every page it worked on,
 * each with its newest frame, how the read went, and whether it waits on the
 * person; a search part carries what it cited.
 */
export function environmentParts(
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
    if (!stage.parts.length) continue;
    const projection = objectives.get(stage.objective);
    if (!projection) continue;
    const recorded = pages(projection.work.id);
    const waiting = human(projection.work.id);
    const paused = activity(projection.work.id) === "paused";
    for (const part of stage.parts) {
      const entries = part.pages.map((entry): PartPage => {
        const opened = recorded.filter((page) => entry.steps.some((step) => step.id === page.step));
        const live =
          part.state === "running" && entry.steps.some((step) => step.status === "running");
        const succeeded = entry.steps.some((step) => step.status === "succeeded");
        const refused = !succeeded && !live ? (entry.steps.at(-1)?.note?.trim() ?? "") : "";
        const frame = entry.page?.frame
          ? pageFrameUrl(entry.page.attempt, entry.page.step, entry.page.frame.generation)
          : null;
        // Rust names a held page by its read step; one page folds several of them.
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
        return {
          id: entry.id,
          url: entry.url,
          title: clipText(observed?.trim() || entry.tab?.title.trim() || "", TITLE_TEXT),
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
          frame,
          live,
          ...(held ? { human: humanPage(held) } : {}),
          ...(account ? { account } : {}),
          ...(shown ? { tab: true } : {}),
        };
      });
      const rect = stage.lane.rects[part.id]!;
      const needs = !!part.ask || entries.some((page) => page.human?.phase === "waiting_for_human");
      // The orb stands at the first part at work; every other part at work has its own small one.
      const helper =
        part.state === "running" &&
        stage.parts.find((candidate) => candidate.state === "running") !== part;
      const state = needs ? "waiting" : part.state;
      const summary = needs ? m.work_line_waiting_for_you() : partSummary(part, stage);
      const rows: NonNullable<CanvasItem["sources"]> =
        part.helper === "research"
          ? part.sources.slice(0, SOURCE_ROWS).map((row) => ({
              key: row.key,
              url: row.url,
              where: host(row.url).replace(/^www\./u, ""),
              title: row.title,
            }))
          : [
              ...entries.map((page) => ({
                key: page.id,
                url: page.url,
                where: host(page.url).replace(/^www\./u, ""),
                title: page.title || page.url,
              })),
              ...part.unread.map((page) => ({
                key: page.key,
                url: page.url,
                where: page.host.replace(/^www\./u, ""),
                title: page.url,
                note: page.note,
              })),
            ];
      items.push({
        id: part.id,
        type: "part",
        kind: m.work_part(),
        title: part.title,
        detail: "",
        status: summary ?? "",
        size: { width: rect.width, height: rect.height },
        part: {
          title: part.title,
          ...(part.host ? { host: part.host } : {}),
          helper: part.helper,
          state,
          shape: partShape(part).kind,
          ...(summary ? { summary } : {}),
          pages: entries,
          objective: stage.objective,
          ...(part.steps ? { steps: part.steps } : {}),
          ...(part.lines ? { lines: part.lines } : {}),
          ...(part.connection ? { connection: part.connection } : {}),
          ...(part.ask ? { ask: part.ask } : {}),
          ...(part.need ? { need: needView(part.need) } : {}),
          ...(stage.notes.get(part.id)?.length
            ? { notes: stage.notes.get(part.id)!.map((object) => object.view) }
            : {}),
          ...(helper ? { presence: agentSeed(stage.objective) } : {}),
          ...(part.helper === "research"
            ? {
                cited: citedSites(part.sources).slice(0, PART.sourceRows),
                citedCount: citedSites(part.sources).length,
              }
            : {}),
        },
        sources: rows,
        ...(state === "running" || state === "waiting" ? { active: true } : {}),
      });
    }
  }
  return items;
}

/** What each run drew on before it began, as marks left of its request. */
export function environmentInputs(stages: readonly WorkStage[]): CanvasItem[] {
  return stages.flatMap((stage) =>
    stage.inputs.map(({ id, input }) => {
      const rect = stage.lane.rects[id]!;
      return {
        id,
        type: "input" as const,
        kind: m.work_input(),
        title: input.label,
        detail: "",
        status: "",
        size: { width: rect.width, height: rect.height },
        input,
      };
    }),
  );
}

/** What each run drew on, under its result. */
export function environmentSources(stages: readonly WorkStage[]): CanvasItem[] {
  return stages.flatMap((stage) => {
    const sources = stage.sources;
    const rect = sources ? stage.lane.rects[sources.id] : undefined;
    if (!sources || !rect) return [];
    return [
      {
        id: sources.id,
        type: "sources" as const,
        kind: m.work_sources(),
        title: m.work_sources(),
        detail: "",
        status: "",
        size: { width: rect.width, height: rect.height },
        drawn: sources.view,
      },
    ];
  });
}

/** Each run's result and what its parts found: the reply at its head, then its objects. */
export function environmentBoards(
  stages: readonly WorkStage[],
  /** The one result a block stands for, when it stands for one: its note and tasks come from it. */
  artifactOf: (id: string) => CanvasItem["artifact"] = () => undefined,
): CanvasItem[] {
  const items: CanvasItem[] = [];
  for (const stage of stages) {
    const size = (id: string) => {
      const rect = stage.lane.rects[id]!;
      return { width: rect.width, height: rect.height };
    };
    const reply = stage.reply;
    const answer = reply ? artifactOf(reply.id) : undefined;
    if (reply && stage.lane.rects[reply.id])
      items.push({
        ...(answer ? { artifact: answer } : {}),
        id: reply.id,
        type: "object",
        kind: reply.view.kind,
        title: clipText(reply.view.kind === "reply" ? reply.view.headline : "", TITLE_TEXT),
        detail: "",
        status: "",
        size: size(reply.id),
        object: { view: reply.view, live: stage.live },
      });
    for (const object of stage.objects) {
      if (!stage.lane.rects[object.id]) continue;
      const artifact = artifactOf(object.id);
      const base = {
        id: object.id,
        kind: object.view.kind,
        title: clipText(object.view.title ?? object.name ?? "", TITLE_TEXT),
        detail: "",
        status: "",
        size: size(object.id),
        ...(artifact ? { artifact } : {}),
      };
      items.push({ ...base, type: "object", object: { view: object.view, live: stage.live } });
    }
  }
  return items;
}

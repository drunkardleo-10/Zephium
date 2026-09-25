import type {
  WorkArtifactV1,
  WorkCommandRecordV1,
  WorkEnvironmentElement,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkFileRecordV1,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { FILE_STEPS, isAgentExecution } from "./agent-steps";
import { clipText, type CanvasItem, type CanvasPosition, type CanvasSize } from "./canvas-model";
import { findingsSize, resultSize, sourcesSize, stepSize, subjectSize } from "./card-size";
import { resultPlan, stepId } from "./plan-steps";
import { artifactView } from "./project-work";
import { CLUSTER_CAP, SIZES, type StageContents, type StageMember } from "./stage-layout";
import { listingArtifacts, subjectFacts, subjectKey, subjectsOf } from "./subjects";
import { fileFolder, fileName, homePath } from "./work-files";
import * as m from "$shared/i18n/messages";

const DETAIL_TEXT = 2048;
const ROW_TEXT = 200;
const TAIL_LINES = 3;

export function host(url: string | undefined): string {
  if (!url) return "";
  try {
    return new URL(url).host;
  } catch {
    return "";
  }
}
export type SourceRow = {
  key: string;
  url: string;
  where: string;
  title: string;
  note?: string;
  file?: { record: string; path: string; kind: string };
};
/** A file a step disclosed: the folder it sits in stands where a host would. */
function fileRow(key: string, record: WorkFileRecordV1, title?: string): SourceRow {
  return {
    key,
    url: "",
    where: clipText(fileFolder(record.file.path), ROW_TEXT),
    title: clipText(title || record.file.name, ROW_TEXT),
    file: { record: record.id, path: record.file.path, kind: record.file.kind },
  };
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
      return [
        {
          key,
          url: clipText(url, DETAIL_TEXT),
          where: host(url),
          title: clipText(entry.title || citation?.title || host(url), ROW_TEXT),
        },
      ];
    // A granted folder is not a place the pane can open: the row names the file.
    const record = execution.file_evidence?.find(
      (candidate) => candidate.id === link.extraction_id,
    );
    return record ? [fileRow(key, record, entry.title)] : [];
  });
}

/** Every page and file one run cited or opened, once each, in step order. */
export function sourceRows(execution: WorkExecutionFact): SourceRow[] {
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
  return rows;
}

type Step = NonNullable<WorkExecutionFact["steps"]>[number];
export type PageGroup = { id: string; url: string; steps: Step[]; page?: WorkPageV1 };
/** One card per page: every step that opened the same URL folds into it. */
export function pageGroups(
  execution: WorkExecutionFact,
  recorded: readonly WorkPageV1[],
): PageGroup[] {
  const opened = recorded.filter((page) => page.execution === execution.id);
  const byUrl = new Map<string, PageGroup>();
  for (const step of execution.steps ?? []) {
    if (step.kind.kind !== "read" && step.kind.kind !== "discover") continue;
    const page = opened.find((page) => page.step === step.id);
    const url = page?.url || (step.kind.kind === "read" ? step.kind.url : "");
    if (!url) continue;
    const entry = byUrl.get(url) ?? { id: `page:${execution.id}:${step.id}`, url, steps: [] };
    entry.steps.push(step);
    if (page?.frame && (!entry.page?.frame || page.live)) entry.page = page;
    else entry.page ??= page;
    byUrl.set(url, entry);
  }
  return [...byUrl.values()];
}

type FileWhat = NonNullable<CanvasItem["file"]>["what"];
const RANK: Record<FileWhat, number> = { read: 0, searched: 1, changed: 2, created: 3 };
const lines = (text: string) => (text ? text.split("\n").length : 0);
/** The files one run touched, one card per path, carrying the strongest thing it did there. */
export function fileCards(execution: WorkExecutionFact): {
  id: string;
  path: string;
  file: NonNullable<CanvasItem["file"]>;
}[] {
  const byPath = new Map<
    string,
    { id: string; path: string; file: NonNullable<CanvasItem["file"]> }
  >();
  for (const step of execution.steps ?? []) {
    if (step.status !== "succeeded") continue;
    const kind = step.kind;
    if (
      kind.kind !== "read_file" &&
      kind.kind !== "search_files" &&
      kind.kind !== "write_file" &&
      kind.kind !== "edit_file"
    )
      continue;
    const known = byPath.has(kind.path);
    const entry = byPath.get(kind.path) ?? {
      // Paths run long; the order a run first touched them names the card.
      id: `file:${execution.id}:${byPath.size}`,
      path: kind.path,
      file: {
        name: clipText(fileName(kind.path), ROW_TEXT),
        folder: clipText(fileFolder(kind.path), ROW_TEXT),
        what: "read" as FileWhat,
      },
    };
    const what: FileWhat =
      kind.kind === "read_file"
        ? "read"
        : kind.kind === "search_files"
          ? "searched"
          : kind.kind === "edit_file" || known
            ? "changed"
            : "created";
    if (RANK[what] > RANK[entry.file.what]) entry.file = { ...entry.file, what };
    if (kind.kind === "write_file" || kind.kind === "edit_file") {
      const delta = entry.file.delta ?? { plus: 0, minus: 0 };
      entry.file = {
        ...entry.file,
        delta:
          kind.kind === "edit_file"
            ? { plus: delta.plus + lines(kind.new), minus: delta.minus + lines(kind.old) }
            : { plus: delta.plus + lines(kind.content), minus: delta.minus },
      };
    }
    byPath.set(kind.path, entry);
  }
  return [...byPath.values()];
}

const tail = (text: string) =>
  text
    .trimEnd()
    .split("\n")
    .slice(-TAIL_LINES)
    .map((line) => clipText(line, ROW_TEXT));
/** Why a command ran, in the person's words; nothing when the policy gives no closed reason. */
function commandReason(step: Step): string | undefined {
  const policy = step.local?.policy;
  if (!policy || step.kind.kind !== "run_command") return undefined;
  const approved = step.kind.decision === true;
  if (policy.class === "read") return m.work_command_ran_without_asking();
  if (policy.class === "write" && (approved || policy.scope === "none"))
    return m.work_command_allowed_folder();
  if (policy.class === "ask" && approved) return m.work_command_you_approved();
  return undefined;
}
/** One card per command step: live output while it runs, its record once it settles. */
export function commandCards(
  execution: WorkExecutionFact,
): { id: string; command: NonNullable<CanvasItem["command"]> }[] {
  const records = execution.command_evidence ?? [];
  const cited = new Set<string>();
  const cards: { id: string; command: NonNullable<CanvasItem["command"]> }[] = [];
  for (const step of execution.steps ?? []) {
    if (step.kind.kind !== "run_command") continue;
    const line = clipText(step.kind.command, ROW_TEXT);
    const reason = commandReason(step);
    const id = `command:${execution.id}:${step.id}`;
    if (step.status === "running") {
      const output = step.local?.output;
      // No output yet means no process: the command waits on the person, without a timer.
      const waiting = !output && step.kind.decision == null && step.local?.policy?.scope !== "none";
      const why = waiting ? m.work_command_waiting() : reason;
      cards.push({
        id,
        command: {
          line,
          state: "running",
          ...(output ? { elapsed_ms: output.elapsed_ms ?? 0 } : {}),
          tail: output ? tail(output.text) : [],
          ...(why ? { reason: why } : {}),
        },
      });
      continue;
    }
    const record = records.find((entry) => entry.id === step.evidence);
    if (!record) continue;
    cited.add(record.id);
    cards.push({ id, command: settled(record, line, reason) });
  }
  for (const record of records)
    if (!cited.has(record.id))
      cards.push({
        id: `command:${execution.id}:${record.id}`,
        command: settled(record, clipText(record.command.command, ROW_TEXT)),
      });
  return cards;
}
function settled(
  record: WorkCommandRecordV1,
  line: string,
  reason?: string,
): NonNullable<CanvasItem["command"]> {
  return {
    line,
    state: "exit",
    ...(typeof record.command.exit === "number" ? { exit: record.command.exit } : {}),
    elapsed_ms: record.command.elapsed_ms,
    tail: tail(record.command.text),
    record: record.id,
    ...(reason ? { reason } : {}),
  };
}
/** The full record behind a settled command card, wherever its run sits on the canvas. */
export function commandRecord(
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  record: string,
): WorkCommandRecordV1 | undefined {
  for (const projection of objectives.values())
    for (const execution of projection.executions)
      for (const entry of execution.command_evidence ?? []) if (entry.id === record) return entry;
  return undefined;
}
/** The title a successful read observed for this address, the latest one across the canvas. */
export function observedTitle(
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  url: string,
): string | undefined {
  const wanted = cleanUrl(url);
  let title: string | undefined;
  for (const projection of objectives.values())
    for (const execution of projection.executions)
      for (const step of execution.steps ?? [])
        if (
          step.kind.kind === "read" &&
          step.status === "succeeded" &&
          step.local?.page_title &&
          cleanUrl(step.kind.url) === wanted
        )
          title = step.local.page_title;
  return title;
}
export const displayPath = (path: string) => clipText(homePath(path), DETAIL_TEXT);

/** A published object's card, sized to what it says, as its canvas item is built. */
export function artifactSize(artifact: WorkArtifactV1, execution: WorkExecutionFact): CanvasSize {
  const view = artifactView(artifact, execution);
  if (view.content.kind !== "findings") return resultSize(artifact.title, view);
  const { items, subjects } = view.content;
  return findingsSize(
    items.map((item) => {
      const subject = item.subject === undefined ? undefined : subjects[item.subject];
      return { claim: item.claim, ...(subject ? { subject: subject.name } : {}) };
    }),
    items.length,
  );
}
/** A subject's card: its picture, its name and the facts the run established. */
export function subjectCardSize(
  execution: WorkExecutionFact,
  subject: ReturnType<typeof subjectsOf>[number],
): CanvasSize {
  return subjectSize(
    subject.name,
    !!subject.image_candidates?.length,
    subjectFacts(execution, subject),
    subject.descriptor ?? "",
  );
}

/**
 * Whether an element's card stands in a lane: a request, or what one of the
 * canvas's works found or made. The person's own elements keep absolute places.
 */
export function laneElement(
  snapshot: WorkEnvironmentSnapshot,
  element: WorkEnvironmentElement,
): boolean {
  const reference = element.reference;
  if (reference.kind === "objective") return true;
  if (reference.kind !== "subject" && reference.kind !== "finding" && reference.kind !== "artifact")
    return false;
  return snapshot.elements.some(
    (candidate) =>
      candidate.reference.kind === "objective" &&
      candidate.reference.objective === reference.objective,
  );
}
/** A placement written in lane terms: `x, y` are an offset from the lane place. */
export const LANE_PLACEMENT = 2;
export type LaneOffset = { offset: CanvasPosition; size: CanvasSize };
/**
 * What the person did to lane cards: moved them by an offset, resized them.
 * A placement from before lanes says neither and is left out, so the card
 * takes its lane place once and is saved in lane terms from then on.
 */
export function laneOffsets(snapshot: WorkEnvironmentSnapshot): Map<string, LaneOffset> {
  const lane = new Set(
    snapshot.elements.flatMap((element) => (laneElement(snapshot, element) ? [element.id] : [])),
  );
  const offsets = new Map<string, LaneOffset>();
  for (const place of snapshot.view.placements)
    if (lane.has(place.element) && place.revision === LANE_PLACEMENT)
      offsets.set(place.element, {
        offset: { x: place.x, y: place.y },
        size: { width: place.width, height: place.height },
      });
  return offsets;
}

/** What a request card says under its words: how long its runs took, what they read. */
export function laneFacts(
  executions: readonly WorkExecutionFact[],
  recorded: readonly WorkPageV1[] = [],
): Pick<CanvasItem, "elapsed" | "counts"> {
  let elapsed = 0;
  let sources = 0;
  let pages = 0;
  for (const execution of executions) {
    for (const step of execution.steps ?? []) elapsed += step.measurements?.wall_millis ?? 0;
    if (!isAgentExecution(execution)) continue;
    sources += sourceRows(execution).length;
    pages += pageGroups(execution, recorded).length;
  }
  return {
    ...(elapsed ? { elapsed } : {}),
    ...(sources || pages ? { counts: { sources, pages } } : {}),
  };
}

/** Lead results first: a comparison or a chart heads the Made group. */
const leads = (artifact: WorkArtifactV1 | undefined) =>
  artifact?.data.kind === "comparison_matrix" || artifact?.data.kind === "chart";

/** What one message's runs put in each group, in projection order. */
export function stageContents(
  snapshot: WorkEnvironmentSnapshot,
  projection: WorkRuntimeProjection,
  stage: { element: string; executions: readonly string[] },
  recorded: readonly WorkPageV1[] = [],
  offsets: ReadonlyMap<string, LaneOffset> = laneOffsets(snapshot),
): StageContents {
  // A card the person resized keeps that size in its lane.
  const member = (id: string, size: CanvasSize): StageMember => ({
    id,
    size: offsets.get(id)?.size ?? size,
  });
  const runs = stage.executions.flatMap((id) => {
    const execution = projection.executions.find((entry) => entry.id === id);
    return execution ? [execution] : [];
  });
  const sources: StageMember[] = [];
  const pages: StageMember[] = [];
  const work: StageMember[] = [];
  for (const execution of runs) {
    if (!isAgentExecution(execution)) continue;
    const rows = sourceRows(execution).length;
    if (rows)
      sources.push({ id: `sources:${stage.element}:${execution.id}`, size: sourcesSize(rows) });
    for (const group of pageGroups(execution, recorded))
      pages.push({ id: group.id, size: SIZES.page });
    for (const card of fileCards(execution)) work.push({ id: card.id, size: SIZES.file });
    for (const card of commandCards(execution)) work.push({ id: card.id, size: SIZES.command });
  }
  const subjects: StageMember[] = [];
  const findings: StageMember[] = [];
  const results: (StageMember & { lead: boolean })[] = [];
  const plan: StageMember[] = [];
  const hubs = new Set<string>();
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (!("execution" in reference) || !stage.executions.includes(reference.execution)) continue;
    const execution = runs.find((run) => run.id === reference.execution);
    const artifact = execution?.artifacts.find((entry) => entry.id === reference.artifact);
    if (reference.kind === "subject") {
      const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
      if (subject) hubs.add(subjectKey(subject));
      subjects.push(
        member(
          element.id,
          subject && execution ? subjectCardSize(execution, subject) : SIZES.subject,
        ),
      );
    } else if (reference.kind === "finding") findings.push(member(element.id, SIZES.findings));
    else if (reference.kind === "artifact") {
      const size = artifact && execution ? artifactSize(artifact, execution) : SIZES.result;
      if (artifact?.data.kind === "findings") {
        findings.push(member(element.id, size));
        continue;
      }
      results.push({ ...member(element.id, size), lead: leads(artifact) });
      // A result's plan stands beside it, one card per step.
      const view = artifact && execution ? artifactView(artifact, execution) : undefined;
      resultPlan(view?.content).forEach((step, index) =>
        plan.push({
          id: stepId(element.id, index),
          size: stepSize(step.text, step.detail),
          of: element.id,
        }),
      );
    }
  }
  // Subjects the runs named that never earned a card only count on the caption.
  const named = new Set<string>();
  for (const execution of runs) {
    const listings = listingArtifacts(execution);
    for (const artifact of execution.artifacts)
      if (!listings.has(artifact.id))
        for (const subject of subjectsOf(artifact)) named.add(subjectKey(subject));
  }
  const unshown = [...named].filter((key) => !hubs.has(key)).length;
  return {
    sources: { members: sources },
    pages: { members: pages },
    work: { members: work },
    subjects: { members: subjects, more: subjects.length >= CLUSTER_CAP.subjects ? unshown : 0 },
    findings: { members: findings },
    results: {
      members: [
        ...results.filter((result) => result.lead),
        ...results.filter((result) => !result.lead),
      ].map(({ id, size }) => ({ id, size })),
    },
    plan: { members: plan },
  };
}

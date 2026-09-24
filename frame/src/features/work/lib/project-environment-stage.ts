import type {
  WorkArtifactV1,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkFileRecordV1,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { FILE_STEPS, isAgentExecution } from "./agent-steps";
import { clipText, type CanvasItem, type CanvasSize } from "./canvas-model";
import { CLUSTER_CAP, SIZES, type StageContents, type StageMember } from "./stage-layout";
import { listingArtifacts, subjectKey, subjectsOf } from "./subjects";
import { fileFolder, fileName, homePath } from "./work-files";

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

/** The command records a run keeps once the local runtime reports them. */
type CommandRecord = {
  id: string;
  command: { command: string; exit?: number | null; elapsed_ms: number; text: string };
};
export function commandCards(
  execution: WorkExecutionFact,
): { id: string; command: NonNullable<CanvasItem["command"]> }[] {
  if (!("command_evidence" in execution) || !Array.isArray(execution.command_evidence)) return [];
  return (execution.command_evidence as CommandRecord[]).map((record) => ({
    id: `command:${execution.id}:${record.id}`,
    command: {
      line: clipText(record.command.command, ROW_TEXT),
      state: "exit",
      ...(typeof record.command.exit === "number" ? { exit: record.command.exit } : {}),
      elapsed_ms: record.command.elapsed_ms,
      tail: record.command.text
        .trimEnd()
        .split("\n")
        .slice(-TAIL_LINES)
        .map((line) => clipText(line, ROW_TEXT)),
    },
  }));
}
export const displayPath = (path: string) => clipText(homePath(path), DETAIL_TEXT);

function resultSize(artifact: WorkArtifactV1 | undefined): CanvasSize {
  switch (artifact?.data.kind) {
    case "findings":
      return SIZES.findings;
    case "document":
      return SIZES.document;
    case "comparison_matrix":
      return SIZES.comparison;
    default:
      return SIZES.result;
  }
}

/** What one message's runs put in each cluster, in projection order. */
export function stageContents(
  snapshot: WorkEnvironmentSnapshot,
  projection: WorkRuntimeProjection,
  stage: { element: string; executions: readonly string[] },
  recorded: readonly WorkPageV1[] = [],
): StageContents {
  const placements = new Map(snapshot.view.placements.map((place) => [place.element, place]));
  const member = (id: string, size: CanvasSize): StageMember => {
    const place = placements.get(id);
    return place
      ? {
          id,
          size: { width: place.width, height: place.height },
          placed: { x: place.x, y: place.y },
        }
      : { id, size };
  };
  const runs = stage.executions.flatMap((id) => {
    const execution = projection.executions.find((entry) => entry.id === id);
    return execution ? [execution] : [];
  });
  const sources: StageMember[] = [];
  const pages: StageMember[] = [];
  const work: StageMember[] = [];
  for (const execution of runs) {
    if (!isAgentExecution(execution)) continue;
    if (sourceRows(execution).length)
      sources.push({ id: `sources:${stage.element}:${execution.id}`, size: SIZES.sources });
    for (const group of pageGroups(execution, recorded))
      pages.push({ id: group.id, size: SIZES.page });
    for (const card of fileCards(execution)) work.push({ id: card.id, size: SIZES.file });
    for (const card of commandCards(execution)) work.push({ id: card.id, size: SIZES.command });
  }
  const subjects: StageMember[] = [];
  const findings: StageMember[] = [];
  const results: StageMember[] = [];
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
        member(element.id, subject?.image_candidates?.length ? SIZES.pictured : SIZES.subject),
      );
    } else if (reference.kind === "finding") findings.push(member(element.id, SIZES.findings));
    else if (reference.kind === "artifact")
      (artifact?.data.kind === "findings" ? findings : results).push(
        member(element.id, resultSize(artifact)),
      );
  }
  // Subjects the runs named that never earned a card only count on the label.
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
    results: { members: results },
  };
}

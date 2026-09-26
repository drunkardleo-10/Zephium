import type {
  WorkArtifactV1,
  WorkCommandRecordV1,
  WorkContextTabV1,
  WorkExecutionFact,
  WorkFileRecordV1,
  WorkPageV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { FILE_STEPS } from "./agent-steps";
import { clipText } from "./canvas-model";
import { fileFolder } from "./work-files";
import * as m from "$shared/i18n/messages";

const DETAIL_TEXT = 2048;
const ROW_TEXT = 200;

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
export type PageGroup = {
  id: string;
  url: string;
  steps: Step[];
  page?: WorkPageV1;
  /** An open tab the person let the request see; it stands as a page until one is read. */
  tab?: WorkContextTabV1;
};
/** Every read of the page settled and none of them read it. */
const failed = (group: PageGroup) =>
  group.steps.length > 0 &&
  group.steps.every((step) => step.status !== "running" && step.status !== "succeeded");
/**
 * One card per page: every step that opened the same URL folds into it. The
 * open tabs the request was given follow, unread, unless a read took one over.
 * A page no read could read is not a card: the Sources card lists it, and a
 * tab the request was shown stays the tab it was.
 */
export function pageGroups(
  execution: WorkExecutionFact,
  recorded: readonly WorkPageV1[],
): PageGroup[] {
  return openedGroups(execution, recorded).flatMap((group) =>
    !failed(group) ? [group] : group.tab ? [{ ...group, steps: [], page: undefined }] : [],
  );
}
export type UnreadPage = { key: string; url: string; host: string; note: string };
/** The pages a run opened and could not read, each with Rust's closed reason. */
export function unreadPages(execution: WorkExecutionFact): UnreadPage[] {
  return openedGroups(execution, [])
    .filter(failed)
    .map((group) => ({
      key: group.id,
      url: clipText(group.url, DETAIL_TEXT),
      host: host(group.url),
      note: clipText(group.steps.at(-1)?.note?.trim() || m.work_env_status_failed(), ROW_TEXT),
    }));
}
function openedGroups(execution: WorkExecutionFact, recorded: readonly WorkPageV1[]): PageGroup[] {
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
  for (const [index, tab] of (execution.spec.context?.tabs ?? []).entries()) {
    const url = `https://${tab.host}${tab.path}`;
    const read = byUrl.get(url);
    if (read) read.tab ??= tab;
    else byUrl.set(url, { id: `page:${execution.id}:tab:${index}`, url, steps: [], tab });
  }
  return [...byUrl.values()];
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

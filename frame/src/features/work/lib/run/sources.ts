import type { WorkExecutionFact, WorkPageV1 } from "$shared/ipc/bindings";
import { pageFrameUrl } from "$domain/resources";
import { clipText } from "../canvas-model";
import { fileFolder } from "../work-files";
import { sourceRows, unreadPages } from "../project-environment-stage";
import * as m from "$shared/i18n/messages";

/** One page or file a run drew on: its site and its title. */
export type RunSource = {
  key: string;
  url: string;
  /** The site without `www.`, or the folder a file sits in. */
  host: string;
  title: string;
  /** The page's newest captured frame, when the run opened it. */
  frame?: string;
  file?: { record: string; path: string };
};
export type RunSources = {
  rows: RunSource[];
  /** Pages the run opened and could not read, each with Rust's closed reason. */
  unread: { key: string; url: string; host: string; note: string }[];
};

const TEXT = 200;
const URL_TEXT = 2048;

function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./u, "");
  } catch {
    return "";
  }
}

/** The address as a person would share it: no tracking tail, no fragment. */
function same(url: string): string {
  try {
    const parsed = new URL(url);
    parsed.hash = "";
    for (const key of [...parsed.searchParams.keys()])
      if (key.startsWith("utm_")) parsed.searchParams.delete(key);
    return parsed.toString().replace(/\/$/u, "");
  } catch {
    return url;
  }
}

type Execution = WorkExecutionFact;

/** A page's own words for itself from its address: a search's query, else its last path segment. */
function addressName(url: string): string {
  try {
    const parsed = new URL(url);
    const query = ["q", "query", "search", "k", "keywords"]
      .map((key) => parsed.searchParams.get(key)?.trim())
      .find(Boolean);
    if (query) return query;
    const segments = parsed.pathname.split("/").filter(Boolean).map(decodeURIComponent);
    const last = segments.at(-1) ?? "";
    // A numbered page is named with what it is a number of: "issues 123".
    const numbered = /^\d+$/u.test(last) && segments.length > 1;
    const words = (text: string) => text.replace(/[-_+]+/gu, " ").trim();
    const segment = numbered
      ? `${words(segments.at(-2)!)} ${last}`
      : words(last.replace(/\.[a-z0-9]{2,5}$/iu, "")).replace(/(?:\s\d{3,})+$/u, "");
    if (!segment || /^\d+$/u.test(segment) || segment.length < 3) return "";
    return segment.charAt(0).toLocaleUpperCase() + segment.slice(1);
  } catch {
    return "";
  }
}

/**
 * What a page read in a run is called, never its host said twice: the title
 * the page gave, the list it was read for, the name of what the run took from
 * it, a citation's title for the same address, the words in its address.
 */
export function pageName(runs: readonly Execution[], url: string, step?: string): string {
  const at = same(url);
  for (const run of runs) {
    const read = (run.steps ?? []).find(
      (entry) =>
        entry.kind.kind === "read" && (entry.id === step || (!step && same(entry.kind.url) === at)),
    );
    const titled = read?.local?.page_title?.trim();
    if (titled) return titled;
    if (read?.kind.kind === "read" && read.kind.collection?.title?.trim())
      return read.kind.collection.title.trim();
  }
  for (const run of runs)
    for (const artifact of run.artifacts) {
      const data = artifact.data;
      if (data.kind !== "picks") continue;
      const item = data.items.find((entry) => entry.url && same(entry.url) === at);
      if (item?.name.trim()) return item.name.trim();
    }
  for (const run of runs)
    for (const record of run.provider_evidence ?? [])
      for (const citation of record.evidence.citations)
        if (citation.url && same(citation.url) === at && citation.title?.trim())
          return citation.title.trim();
  return addressName(url);
}
/** What a search or a listing of a folder found is not a source; the files it led to are. */
const LISTINGS = new Set(["search", "list", "listing", "directory"]);
type Link = { extraction_id: string; source_id: number };

/** A citation a search step or an object names: its page, or the file a folder gave. */
function resolve(execution: Execution, link: Link): RunSource | null {
  const key = `${link.extraction_id}:${link.source_id}`;
  const record = execution.provider_evidence?.find((entry) => entry.id === link.extraction_id);
  const citation = record?.evidence.citations[link.source_id - 1];
  if (citation?.url) {
    const host = hostOf(citation.url);
    if (!host) return null;
    return {
      key,
      url: clipText(citation.url, URL_TEXT),
      host,
      title: clipText(citation.title?.trim() || addressName(citation.url) || host, TEXT),
    };
  }
  const file = execution.file_evidence?.find((entry) => entry.id === link.extraction_id);
  return file && !LISTINGS.has(file.file.kind)
    ? {
        key,
        url: "",
        host: clipText(fileFolder(file.file.path), TEXT),
        title: clipText(file.file.name, TEXT),
        file: { record: file.id, path: file.file.path },
      }
    : null;
}

/**
 * What a run drew on, for the Sources under its result: first what its
 * objects cite, then the pages it read, then what its searches found, each
 * page once. Pages it opened and could not read are kept apart, one quiet
 * line under the rest.
 */
export function runSources(
  runs: readonly Execution[],
  recorded: readonly WorkPageV1[] = [],
): RunSources {
  const rows: RunSource[] = [];
  const seen = new Map<string, RunSource>();
  const files = new Set<string>();
  const admit = (row: RunSource | null) => {
    if (!row) return;
    if (row.file) {
      if (files.has(row.file.path)) return;
      files.add(row.file.path);
      rows.push(row);
      return;
    }
    const at = same(row.url);
    // The same page under two addresses (http and https, a trailing index) is one row.
    const named = row.title !== row.host ? `${row.host}|${row.title.toLowerCase()}` : at;
    const known = seen.get(at) ?? seen.get(named);
    if (known) {
      if (!known.frame && row.frame) known.frame = row.frame;
      seen.set(at, known);
      return;
    }
    seen.set(at, row);
    seen.set(named, row);
    rows.push(row);
  };
  for (const run of runs)
    for (const artifact of run.artifacts)
      for (const link of artifact.evidence ?? []) admit(resolve(run, link));
  for (const run of runs)
    for (const step of run.steps ?? []) {
      if (step.kind.kind !== "read" || step.status !== "succeeded") continue;
      const pages = recorded.filter((page) => page.execution === run.id && page.step === step.id);
      const page = pages.find((entry) => entry.frame) ?? pages[0];
      const url = page?.url || step.kind.url;
      const host = hostOf(url);
      if (!host) continue;
      const frame = page?.frame
        ? pageFrameUrl(page.attempt, page.step, page.frame.generation)
        : null;
      admit({
        key: `page:${run.id}:${step.id}`,
        url: clipText(url, URL_TEXT),
        host,
        title: clipText(pageName(runs, url, step.id) || host, TEXT),
        ...(frame ? { frame } : {}),
      });
    }
  for (const run of runs) {
    for (const step of run.steps ?? []) {
      if (step.kind.kind !== "search" || !step.evidence) continue;
      const record = run.provider_evidence?.find((entry) => entry.id === step.evidence);
      record?.evidence.citations.forEach((_, index) =>
        admit(resolve(run, { extraction_id: record.id, source_id: index + 1 })),
      );
    }
    // A run from before the lead cites through its evidence collections.
    for (const row of sourceRows(run))
      if (!row.file || !LISTINGS.has(row.file.kind))
        admit(
          row.file
            ? { key: row.key, url: "", host: row.where, title: row.title, file: row.file }
            : {
                key: row.key,
                url: row.url,
                host: row.where.replace(/^www\./u, ""),
                title: row.title,
              },
        );
  }
  const unread: RunSources["unread"] = [];
  const missed = new Set<string>();
  for (const run of runs)
    for (const page of unreadPages(run)) {
      const at = same(page.url);
      if (seen.has(at) || missed.has(at)) continue;
      missed.add(at);
      unread.push({
        key: page.key,
        url: page.url,
        host: page.host.replace(/^www\./u, ""),
        note: page.note || m.work_env_status_failed(),
      });
    }
  return { rows, unread };
}

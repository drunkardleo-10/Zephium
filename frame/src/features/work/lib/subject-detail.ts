import type { WorkExecutionFact, WorkPageV1 } from "$shared/ipc/bindings";
import type { EvidenceReference } from "$shared/ui/data/Artifact";
import { artifactView } from "./project-work";
import { compareModel, type CompareModel } from "./compare";
import { restatesSubject, subjectKey, subjectMatrixRows } from "./subjects";

type Subject = { name: string };
type Fact = {
  key: string;
  label: string;
  value: CompareModel["rows"][number]["cells"][number]["value"];
  numeric: boolean;
  evidence: readonly EvidenceReference[];
};
export type SubjectDetail = {
  price?: string;
  facts: Fact[];
  sources: EvidenceReference[];
  /** The sentence a finding about this subject cites each source for, by source key. */
  quotes: Record<string, string>;
};

/** Everything one run established about a subject: its price, every fact it
 * recorded with the sources behind it, and the pages it read about it. */
export function subjectDetail(execution: WorkExecutionFact, subject: Subject): SubjectDetail {
  const facts: Fact[] = [];
  const sources: EvidenceReference[] = [];
  const seen = new Set<string>();
  const cited = new Set<string>();
  const quotes: Record<string, string> = {};
  const key = subjectKey(subject);
  let price: string | undefined;
  const cite = (reference: EvidenceReference) => {
    if (cited.has(reference.key)) return;
    cited.add(reference.key);
    sources.push(reference);
  };
  for (const { artifact, index } of subjectMatrixRows(execution, subject)) {
    const view = artifactView(artifact, execution);
    if (view.content.kind !== "matrix") continue;
    const row = view.content.cells[index];
    const named = view.content.subjects[index];
    if (!row || !named) continue;
    const one = compareModel({
      subjects: [named],
      criteria: view.content.criteria,
      cells: [row],
      notes: [],
    });
    price ??= one.columns[0]?.price;
    for (const entry of one.rows) {
      const cell = entry.cells[0];
      const key = entry.label.toLocaleLowerCase();
      if (!cell || seen.has(key) || cell.value.kind === "unknown") continue;
      if (cell.value.kind === "text" && restatesSubject(entry.label, cell.value.text, subject.name))
        continue;
      seen.add(key);
      facts.push({
        key,
        label: entry.label,
        value: cell.value,
        numeric: entry.numeric,
        evidence: cell.evidence,
      });
      for (const source of cell.evidence) cite(source);
    }
  }
  // Pages a search cited for this subject, or a finding about it, belong in
  // the list even when no single cell used them; a finding lends its sentence.
  for (const artifact of execution.artifacts) {
    const view = artifactView(artifact, execution);
    const content = view.content;
    if (content.kind === "sources") {
      for (const entry of content.entries)
        if (entry.subject === undefined || content.subjects[entry.subject]?.name === subject.name)
          cite(entry.evidence);
    } else if (content.kind === "findings") {
      for (const item of content.items) {
        const about = item.subject === undefined ? undefined : content.subjects[item.subject];
        if (!about || subjectKey(about) !== key) continue;
        for (const source of item.evidence) {
          cite(source);
          quotes[source.key] ??= item.claim;
        }
      }
    }
  }
  return { facts, sources, quotes, ...(price ? { price } : {}) };
}

const bare = (url: string) => {
  try {
    const parsed = new URL(url);
    return `${parsed.host.replace(/^www\./u, "")}${parsed.pathname.replace(/\/+$/u, "")}${parsed.search}`;
  } catch {
    return url;
  }
};

/**
 * The frame the run captured of the page a subject stands on: its homepage
 * when the run read it, else the first cited page it read, else any page it
 * read on the subject's own site.
 */
export function subjectPage(
  pages: readonly WorkPageV1[],
  homepage: string | undefined,
  sources: readonly EvidenceReference[],
): WorkPageV1 | undefined {
  const framed = pages.filter((page) => page.frame);
  if (!framed.length) return undefined;
  const at = (url: string | undefined) =>
    url ? framed.find((page) => bare(page.url) === bare(url)) : undefined;
  const host = (url: string) => bare(url).split("/")[0];
  const site = homepage ? host(homepage) : null;
  return (
    at(homepage) ??
    sources.map((source) => at(source.url)).find(Boolean) ??
    (site ? framed.find((page) => host(page.url) === site) : undefined)
  );
}

/** A subject as the person's note: its name, what it is, its facts, then its sources. */
export function subjectMarkdown(
  subject: { name: string; descriptor?: string | null; homepage?: string | null },
  detail: SubjectDetail,
  text: (value: Fact["value"]) => string,
  sourcesHeading: string,
): string {
  const line = (value: string) => value.replace(/\s+/gu, " ").trim();
  const parts = [`# ${line(subject.name)}`];
  const lead = [detail.price, subject.descriptor].filter(Boolean).map((value) => line(value!));
  if (lead.length) parts.push(lead.join(" · "));
  if (subject.homepage) parts.push(subject.homepage);
  const facts = detail.facts
    .map((fact) => [fact.label, text(fact.value)] as const)
    .filter(([, value]) => value)
    .map(([label, value]) => `- **${line(label)}:** ${line(value)}`);
  if (facts.length) parts.push(facts.join("\n"));
  const sources = detail.sources
    .filter((source) => source.url)
    .map((source) => `- [${line(source.label || source.origin || source.url!)}](${source.url})`);
  if (sources.length) parts.push(`## ${sourcesHeading}`, sources.join("\n"));
  return `${parts.join("\n\n")}\n`;
}

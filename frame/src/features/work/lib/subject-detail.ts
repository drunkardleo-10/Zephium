import type { WorkExecutionFact } from "$shared/ipc/bindings";
import type { EvidenceReference } from "$shared/ui/data/Artifact";
import { artifactView } from "./project-work";
import { compareModel, type CompareModel } from "./compare";
import { restatesSubject, subjectMatrixRows } from "./subjects";

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
};

/** Everything one run established about a subject: its price, every fact it
 * recorded with the sources behind it, and the pages it read about it. */
export function subjectDetail(execution: WorkExecutionFact, subject: Subject): SubjectDetail {
  const facts: Fact[] = [];
  const sources: EvidenceReference[] = [];
  const seen = new Set<string>();
  const cited = new Set<string>();
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
  // Pages a search cited for this subject belong in the list even when no
  // single cell used them.
  for (const artifact of execution.artifacts) {
    const view = artifactView(artifact, execution);
    if (view.content.kind !== "sources") continue;
    for (const entry of view.content.entries)
      if (
        entry.subject === undefined ||
        view.content.subjects[entry.subject]?.name === subject.name
      )
        cite(entry.evidence);
  }
  return { facts, sources, ...(price ? { price } : {}) };
}

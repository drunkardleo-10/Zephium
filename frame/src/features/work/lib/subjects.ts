import type { WorkArtifactDataV1, WorkArtifactV1, WorkExecutionFact } from "$shared/ipc/bindings";

type Subject = {
  name: string;
  homepage?: string | null;
  image_candidates?: readonly string[] | null;
};
type Matrix = Extract<WorkArtifactDataV1, { kind: "comparison_matrix" }>;
export type SubjectFact = { label: string; value: string };

const DASHES = /[‐-―−­]/g;
const APOSTROPHES = /[‘’‚‛′ʼ]/g;
const QUOTES = /[“”„‟″]/g;

/**
 * Subjects merge across the artifacts of one run by name alone. One run
 * describes the same thing with a product URL, no URL, and the catalogue page
 * it came from; a homepage would split one name or merge two.
 */
export function subjectKey(subject: Subject): string {
  return `name:${normalizeName(subject.name)}`;
}

function normalizeName(name: string): string {
  return name
    .normalize("NFKC")
    .replace(DASHES, "-")
    .replace(APOSTROPHES, "'")
    .replace(QUOTES, '"')
    .toLowerCase()
    .replace(/\s+/gu, " ")
    .trim()
    .replace(/\p{P}+$/u, "")
    .trim();
}

/**
 * A criterion column reduced to its bare label, so "Piece_Count" and
 * "piece count" are one column and "Product URL" is recognisably a link.
 */
function factLabel(name: string): string {
  return name.normalize("NFKC").toLowerCase().replace(/[_-]+/g, " ").replace(/\s+/g, " ").trim();
}

/**
 * A value reduced to its letters and digits, for asking whether a cell only
 * repeats the subject's own name. Trademark marks, punctuation and spacing
 * differ between a catalogue row and the name it names.
 */
function identityText(value: string): string {
  return normalizeName(value).replace(/[^\p{L}\p{N}]+/gu, "");
}

/** Columns that restate who the subject is, or link to it rather than describe it. */
function identityColumn(label: string): boolean {
  return (
    label === "title" ||
    label === "name" ||
    label === "url" ||
    label === "link" ||
    label.endsWith(" url") ||
    label.endsWith(" link")
  );
}

/** The subjects one artifact names, whatever shape carries them. */
export function subjectsOf(artifact: WorkArtifactV1): readonly Subject[] {
  return artifact.data.kind === "comparison_matrix" ||
    artifact.data.kind === "findings" ||
    artifact.data.kind === "evidence_collection"
    ? (artifact.data.subjects ?? [])
    : [];
}

/**
 * Artifacts a browser step recorded: their subjects were observed on the page
 * itself, so their pictures are the ones worth admitting.
 */
export function recordArtifacts(execution: WorkExecutionFact): Set<string> {
  const ids = new Set<string>();
  for (const step of execution.steps ?? [])
    if (step.kind.kind === "read" || step.kind.kind === "discover")
      for (const artifact of step.artifacts ?? []) ids.add(artifact);
  return ids;
}

/**
 * Record artifacts that list rather than establish: a browser step that came
 * back with more than one subject read a catalogue. Its rows enrich hubs the
 * run has for another reason — a detail page read, a comparison, findings —
 * and never become cards of their own, or one listing floods the canvas.
 */
export function listingArtifacts(execution: WorkExecutionFact): Set<string> {
  const records = recordArtifacts(execution);
  const ids = new Set<string>();
  for (const artifact of execution.artifacts)
    if (records.has(artifact.id) && subjectsOf(artifact).length > 1) ids.add(artifact.id);
  return ids;
}

type Row = { matrix: Matrix; index: number; rank: number };
/**
 * Rows about one subject, best evidence first: a detail page read, then what
 * the run published, then catalogue rows. A listing cell says "$349.99 New"
 * where the product page says "$349.99", so it may only fill a criterion no
 * better artifact answered.
 */
function rows(execution: WorkExecutionFact, key: string): Row[] {
  const listings = listingArtifacts(execution);
  const records = recordArtifacts(execution);
  const out: Row[] = [];
  for (const artifact of execution.artifacts) {
    const matrix = artifact.data;
    if (matrix.kind !== "comparison_matrix") continue;
    const rank = listings.has(artifact.id) ? 2 : records.has(artifact.id) ? 0 : 1;
    matrix.subjects.forEach((subject, index) => {
      if (subjectKey(subject) === key && matrix.cells[index]) out.push({ matrix, index, rank });
    });
  }
  return out.sort((a, b) => a.rank - b.rank);
}

/** Everything the run has established about one subject, price first. */
export function subjectFacts(execution: WorkExecutionFact, subject: Subject): SubjectFact[] {
  const money: SubjectFact[] = [];
  const other: SubjectFact[] = [];
  const seen = new Set<string>();
  const identity = identityText(subject.name);
  for (const { matrix, index } of rows(execution, subjectKey(subject))) {
    matrix.criteria.forEach((criterion, column) => {
      const label = factLabel(criterion.name);
      const cell = matrix.cells[index]?.[column];
      if (!cell || seen.has(label) || identityColumn(label)) return;
      const value = cell.value;
      let text: string | null = null;
      switch (value.kind) {
        case "money":
          text = formatMoney(value.amount, value.currency);
          break;
        case "text":
          text = value.text.trim() && !/^https?:\/\//i.test(value.text) ? value.text.trim() : null;
          break;
        case "measurement":
          text =
            criterion.kind.kind === "measurement"
              ? `${value.value} ${criterion.kind.unit}`
              : value.value;
          break;
        case "rating":
          text =
            criterion.kind.kind === "rating"
              ? `${value.value}/${criterion.kind.scale_max}`
              : String(value.value);
          break;
        default:
          break;
      }
      if (!text || identityText(text) === identity) return;
      seen.add(label);
      (value.kind === "money" ? money : other).push({
        label: criterion.name,
        value: text.slice(0, 80),
      });
    });
  }
  return [...money, ...other].slice(0, 3);
}

/**
 * Public image candidates the run has observed for one subject. Candidates a
 * browser step recorded come first: a comparison table names the run's pictures
 * from memory and can cite another subject's.
 */
export function subjectImageCandidates(execution: WorkExecutionFact, subject: Subject): string[] {
  const key = subjectKey(subject);
  const records = recordArtifacts(execution);
  const observed: string[] = [];
  const claimed: string[] = [];
  for (const artifact of execution.artifacts) {
    const out =
      records.has(artifact.id) || artifact.data.kind === "evidence_collection" ? observed : claimed;
    for (const candidate of subjectsOf(artifact)) {
      if (subjectKey(candidate) !== key) continue;
      for (const url of candidate.image_candidates ?? []) if (!out.includes(url)) out.push(url);
    }
  }
  return [...observed, ...claimed.filter((url) => !observed.includes(url))];
}

/** Money as a person reads it here, with the currency the run recorded. */
export function formatMoney(amount: string, currency: string): string {
  const value = Number(amount);
  if (!Number.isFinite(value)) return `${amount} ${currency}`;
  try {
    return new Intl.NumberFormat(undefined, { style: "currency", currency }).format(value);
  } catch {
    return `${amount} ${currency}`;
  }
}

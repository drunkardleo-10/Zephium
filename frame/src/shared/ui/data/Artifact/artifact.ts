import type { ChartSeries } from "../Chart";
/** Structural view of the constrained note schema; the owner supplies the wire value. */
type DocumentMarkView = { type: string; attrs?: { href?: string | null } | null };
export type DocumentNodeView = {
  type: string;
  content?: DocumentNodeView[];
  text?: string | null;
  attrs?: { level?: number | null; start?: number | null; resource?: string | null } | null;
  marks?: DocumentMarkView[];
};
export type NoteDocumentView = { version: number; document: DocumentNodeView };

/** Rendering inputs only. The runtime adapter owns wire validation, identities and permissions. */
export type EvidenceReference = { key: string; label: string; origin?: string; url?: string };
export type SubjectView = {
  name: string;
  descriptor?: string;
  homepage?: string;
  imageCandidates?: readonly string[];
};
export type CriterionView = {
  name: string;
  kind: "text" | "measurement" | "rating" | "presence";
  unit?: string;
  basis?: string;
  rubric?: string;
  scaleMax?: number;
};
type CellValueView =
  | { kind: "text"; text: string }
  | { kind: "measurement"; value: string }
  | { kind: "money"; amount: string; currency: string; observedAt?: string }
  | { kind: "rating"; value: number }
  | { kind: "presence"; present: boolean }
  | { kind: "unknown" };
export type CellView = {
  value: CellValueView;
  evidence: readonly EvidenceReference[];
  note?: string;
  generalKnowledge: boolean;
};
export type FindingView = {
  claim: string;
  subject?: number;
  evidence: readonly EvidenceReference[];
  confidence: "supported" | "inferred" | "unverified" | "contradicted";
  detail?: string;
  generalKnowledge: boolean;
};
export type SourceEntryView = {
  evidence: EvidenceReference;
  title: string;
  role: string;
  subject?: number;
};
type MeasurementBasisView = {
  method: string;
  conditions?: string;
  versions?: string;
  observedAt?: string;
};
export type ArtifactContent =
  | { kind: "document"; paragraphs: readonly string[]; formatted?: NoteDocumentView | null }
  | { kind: "table"; columns: readonly string[]; rows: readonly (readonly string[])[] }
  | {
      kind: "comparison";
      criteria: readonly string[];
      alternatives: readonly { name: string; values: readonly string[] }[];
    }
  | {
      kind: "matrix";
      subjects: readonly SubjectView[];
      criteria: readonly CriterionView[];
      cells: readonly (readonly CellView[])[];
      notes: readonly string[];
    }
  | { kind: "findings"; subjects: readonly SubjectView[]; items: readonly FindingView[] }
  | {
      kind: "chart";
      xLabel: string;
      yLabel: string;
      series: readonly ChartSeries[];
      basis?: MeasurementBasisView;
      generalKnowledge?: boolean;
    }
  | { kind: "checklist"; items: readonly { text: string; completed: boolean }[] }
  | {
      kind: "sources";
      summary: string;
      subjects: readonly SubjectView[];
      entries: readonly SourceEntryView[];
    }
  | { kind: "browser"; title: string; location: string; summary: string }
  | { kind: "unavailable"; reason: string };
export type ArtifactView = {
  key: string;
  title: string;
  content: ArtifactContent;
  reviewLabel: string;
  evidence: readonly EvidenceReference[];
};

// Renderer ceilings, not runtime admission rules. Oversized inputs remain unavailable
// rather than silently presenting a truncated result as the complete artifact.
const text = (value: string) => typeof value === "string" && value.length <= 16_384;
const strings = (values: readonly string[], max: number) =>
  values.length <= max && values.every(text);
const evidenceOk = (refs: readonly EvidenceReference[]) =>
  refs.length <= 128 &&
  new Set(refs.map((item) => item.key)).size === refs.length &&
  refs.every((item) => item.key.length <= 128 && item.label.length <= 512);
const subjectsOk = (subjects: readonly SubjectView[], max = 32) =>
  subjects.length <= max &&
  subjects.every(
    (subject) =>
      text(subject.name) &&
      subject.name.length <= 256 &&
      (subject.descriptor === undefined || subject.descriptor.length <= 256) &&
      (subject.homepage === undefined || subject.homepage.length <= 2048),
  );
export function artifactRenderable(view: ArtifactView): boolean {
  if (
    view.key.length > 128 ||
    view.title.length > 512 ||
    !text(view.title) ||
    !text(view.reviewLabel) ||
    !evidenceOk(view.evidence)
  )
    return false;
  const data = view.content;
  switch (data.kind) {
    case "document":
      return (
        strings(data.paragraphs, 512) &&
        data.paragraphs.reduce((n, p) => n + p.length, 0) <= 262_144
      );
    case "table":
      return (
        strings(data.columns, 32) &&
        data.rows.length <= 10_000 &&
        data.rows.every((row) => row.length === data.columns.length && strings(row, 32)) &&
        data.rows.reduce((n, row) => n + row.reduce((s, cell) => s + cell.length, 0), 0) <=
          1_048_576
      );
    case "comparison":
      return (
        strings(data.criteria, 32) &&
        data.alternatives.length <= 100 &&
        data.alternatives.reduce(
          (n, row) =>
            n + row.name.length + row.values.reduce((total, cell) => total + cell.length, 0),
          0,
        ) <= 262_144 &&
        data.alternatives.every(
          (row) =>
            text(row.name) && row.values.length === data.criteria.length && strings(row.values, 32),
        )
      );
    case "matrix":
      return (
        subjectsOk(data.subjects) &&
        data.subjects.length > 0 &&
        data.criteria.length > 0 &&
        data.criteria.length <= 16 &&
        data.criteria.every((criterion) => text(criterion.name) && criterion.name.length <= 256) &&
        data.cells.length === data.subjects.length &&
        data.cells.every(
          (row) =>
            row.length === data.criteria.length &&
            row.every(
              (cell) =>
                evidenceOk(cell.evidence) &&
                (cell.note === undefined || cell.note.length <= 512) &&
                (cell.value.kind !== "text" || text(cell.value.text)),
            ),
        ) &&
        strings(data.notes, 8)
      );
    case "findings":
      return (
        subjectsOk(data.subjects) &&
        data.items.length > 0 &&
        data.items.length <= 64 &&
        data.items.every(
          (item) =>
            text(item.claim) &&
            item.claim.length <= 1024 &&
            evidenceOk(item.evidence) &&
            (item.subject === undefined || item.subject < data.subjects.length) &&
            (item.detail === undefined || item.detail.length <= 4096),
        )
      );
    case "chart":
      return (
        text(data.xLabel) &&
        text(data.yLabel) &&
        data.series.length <= 16 &&
        data.series.reduce(
          (n, series) =>
            n +
            series.points.reduce(
              (total, point) => total + point.label.length + point.value.length,
              0,
            ),
          0,
        ) <= 65_536 &&
        data.series.every(
          (series) =>
            text(series.name) &&
            series.points.length <= 256 &&
            series.points.every((point) => text(point.label) && text(point.value)),
        )
      );
    case "checklist":
      return (
        data.items.length <= 512 &&
        data.items.reduce((n, item) => n + item.text.length, 0) <= 262_144 &&
        data.items.every((item) => text(item.text) && typeof item.completed === "boolean")
      );
    case "sources":
      return (
        text(data.summary) &&
        subjectsOk(data.subjects) &&
        data.entries.length <= 64 &&
        data.entries.every(
          (entry) =>
            text(entry.title) &&
            entry.title.length <= 512 &&
            entry.role.length <= 128 &&
            (entry.subject === undefined || entry.subject < data.subjects.length),
        )
      );
    case "browser":
      return text(data.title) && text(data.location) && text(data.summary);
    case "unavailable":
      return text(data.reason);
    default:
      return false;
  }
}

/** Text only: a URL in an artifact is never a navigation or browser-context grant. */
export function displayLocation(value: string): string {
  try {
    const url = new URL(value);
    return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password
      ? url.origin
      : "";
  } catch {
    return "";
  }
}

/** Hostname for a source label; empty when the URL is not a plain web address. */
export function displayHost(value: string | undefined): string {
  if (!value) return "";
  try {
    const url = new URL(value);
    return ["https:", "http:"].includes(url.protocol) ? url.hostname.replace(/^www\./, "") : "";
  } catch {
    return "";
  }
}

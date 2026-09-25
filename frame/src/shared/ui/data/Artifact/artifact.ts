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
export type EvidenceReference = {
  key: string;
  label: string;
  origin?: string;
  url?: string;
  /** A file the run read; the owner opens it rather than a web address. */
  file?: { record: string; path: string };
};
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
/** A Work chart as the runtime states it: exact decimals, drawn through the shared chart spec. */
export type WorkChartSeriesView = {
  name: string;
  points: readonly { label: string; value: string; evidence?: readonly EvidenceReference[] }[];
};
export type MeasurementBasisView = {
  method: string;
  conditions?: string;
  versions?: string;
  observedAt?: string;
};
/** A system as the agent drew it: parts, what flows between them, and optional layers. */
type DiagramNodeView = {
  id: string;
  name: string;
  kind: string;
  /** A bare public host, only ever used to draw the vendor's mark. */
  vendor?: string;
  note?: string;
  layer?: string;
};
type DiagramEdgeView = { from: string; to: string; label?: string };
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
      series: readonly WorkChartSeriesView[];
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
  | {
      kind: "diagram";
      nodes: readonly DiagramNodeView[];
      edges: readonly DiagramEdgeView[];
      layers: readonly { id: string; name: string }[];
    }
  | { kind: "unavailable"; reason: string };
export type ArtifactView = {
  key: string;
  title: string;
  content: ArtifactContent;
  reviewLabel: string;
  evidence: readonly EvidenceReference[];
  /** Drawn from what the agent knows, not from a source: a caption stands for the chips. */
  knowledge?: boolean;
};

/** A card carries one knowledge caption: the whole result's, or its chart's. */
export const cardKnowledge = (view: ArtifactView) =>
  !!view.knowledge || (view.content.kind === "chart" && !!view.content.generalKnowledge);

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
    case "diagram": {
      const ids = new Set(data.nodes.map((node) => node.id));
      return (
        data.nodes.length > 0 &&
        data.nodes.length <= 64 &&
        ids.size === data.nodes.length &&
        data.nodes.every(
          (node) =>
            node.id.length <= 64 &&
            node.name.length <= 256 &&
            (node.note === undefined || node.note.length <= 512) &&
            (node.vendor === undefined || node.vendor.length <= 253),
        ) &&
        data.edges.length <= 256 &&
        data.edges.every(
          (edge) =>
            ids.has(edge.from) &&
            ids.has(edge.to) &&
            (edge.label === undefined || edge.label.length <= 128),
        ) &&
        data.layers.length <= 16 &&
        data.layers.every((layer) => layer.name.length <= 128)
      );
    }
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

/** One step of a plan: what to do, and for a numbered section, the first thing it says. */
export type PlanStep = { text: string; detail?: string };
/** What a result card says of a document: its lead, its sections, and the steps it asks for. */
export type DocumentDigest = {
  lead: string;
  /** Section headings the card lists; the summary, the steps and the title are not among them. */
  headings: string[];
  stepsLabel: string;
  steps: PlanStep[];
};
const LEAD_LABEL = /^(summary|in short|tl;?dr|overview|answer|bottom line|the short answer)$/iu;
const STEPS_LABEL =
  /^((recommended |suggested )?next steps?|what to do next|action items|to do|todo|steps)$/iu;
const BULLET = /^\s*(?:[-*•]|\d+[.)]|\[[ xX]\])\s+/u;
const NUMBERED = /^\d{1,2}[.)]\s+/u;
/** Trailing citation markers ("[3, 5]", "[55–56]") belong to the lift, not a card. */
const CITATION = /\s*\[[\d\s,–-]+\]\s*$/u;
const label = (text: string) =>
  text
    .replace(/^#+\s*/u, "")
    .replace(/[:：]\s*$/u, "")
    .replace(/\*+/gu, "")
    .trim();
const bare = (text: string) => text.replace(/\s+/gu, " ").replace(CITATION, "").trim();
const firstSentence = (text: string) => bare(text).split(/(?<=[.!?])\s+/u)[0] ?? "";
function plainText(node: DocumentNodeView): string {
  if (node.type === "text") return node.text ?? "";
  if (node.type === "hardBreak") return "\n";
  return (node.content ?? []).map(plainText).join(node.type === "listItem" ? " " : "");
}

export function documentDigest(content: {
  paragraphs: readonly string[];
  formatted?: NoteDocumentView | null;
}): DocumentDigest {
  // One flat run of blocks: headings (or a paragraph that only names a
  // section), prose, and list items, whichever form the writer used.
  type Block = { kind: "heading" | "text" | "item"; text: string; level?: number };
  const blocks: Block[] = [];
  const push = (text: string) => {
    const lines = text.split("\n").filter((line) => line.trim());
    for (const line of lines) {
      const clean = line.trim();
      const hashes = /^(#+)\s/u.exec(clean)?.[1]?.length;
      if (hashes) blocks.push({ kind: "heading", text: label(clean), level: hashes });
      else if (BULLET.test(clean)) blocks.push({ kind: "item", text: clean.replace(BULLET, "") });
      else if (LEAD_LABEL.test(label(clean)) || STEPS_LABEL.test(label(clean)))
        blocks.push({ kind: "heading", text: label(clean), level: 2 });
      else blocks.push({ kind: "text", text: clean });
    }
  };
  const root = content.formatted?.document;
  if (root)
    for (const node of root.content ?? []) {
      if (node.type === "heading")
        blocks.push({
          kind: "heading",
          text: label(plainText(node)),
          level: node.attrs?.level ?? 2,
        });
      else if (node.type === "bulletList" || node.type === "orderedList")
        for (const item of node.content ?? [])
          blocks.push({ kind: "item", text: plainText(item).replace(/\s+/gu, " ").trim() });
      else if (node.type === "paragraph" || node.type === "blockquote") push(plainText(node));
    }
  else for (const paragraph of content.paragraphs) push(paragraph);
  const heading = (block: Block) => block.kind === "heading";
  const after = (at: number) => {
    const end = blocks.findIndex((block, index) => index > at && heading(block));
    return blocks.slice(at + 1, end < 0 ? undefined : end);
  };
  const find = (pattern: RegExp) =>
    blocks.findIndex((block) => heading(block) && pattern.test(block.text));
  const summary = find(LEAD_LABEL);
  const next = find(STEPS_LABEL);
  const leadBlock =
    (summary >= 0 ? after(summary).find((block) => block.kind === "text") : undefined) ??
    blocks.find((block) => block.kind === "text") ??
    blocks.find((block) => block.kind === "item");
  const lead = leadBlock ? bare(leadBlock.text) : "";
  // A "Next steps" section wins; otherwise sections numbered 1., 2., ... are the plan.
  const numbered = blocks.flatMap((block, index) =>
    heading(block) && (block.level ?? 2) > 1 && NUMBERED.test(block.text) ? [index] : [],
  );
  let steps: PlanStep[] = [];
  if (next >= 0)
    steps = after(next)
      .map((block) => ({ text: bare(block.text) }))
      .filter((step) => step.text && step.text !== lead);
  else if (numbered.length >= 2)
    steps = numbered.map((at) => {
      const first = after(at).find((block) => block.kind !== "heading");
      const detail = first ? firstSentence(first.text) : "";
      return { text: blocks[at]!.text.replace(NUMBERED, ""), ...(detail ? { detail } : {}) };
    });
  const headings = blocks.flatMap((block, index) =>
    heading(block) &&
    (block.level ?? 2) > 1 &&
    index !== summary &&
    index !== next &&
    !(next < 0 && numbered.length >= 2 && numbered.includes(index))
      ? [block.text]
      : [],
  );
  return { lead, headings, stepsLabel: next >= 0 ? blocks[next]!.text : "", steps };
}

/** The steps a result asks the person to take: a checklist's items, or a document's plan. */
export function planSteps(content: ArtifactContent): PlanStep[] {
  if (content.kind === "checklist")
    return content.items.flatMap((item) => (item.text.trim() ? [{ text: bare(item.text) }] : []));
  if (content.kind === "document") return documentDigest(content).steps;
  return [];
}

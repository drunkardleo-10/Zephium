import type {
  ArtifactContent,
  DocumentNodeView,
  EvidenceReference,
} from "$shared/ui/data/Artifact";

/**
 * The composition language: one board per request, blocks placed by the
 * renderer from their role, emphasis and group. The runtime will emit these
 * directly; until then `adapter.ts` derives them from a run's artifacts.
 */
export type Emphasis = "hero" | "primary" | "supporting";
type BlockState = "pending" | "streaming" | "ready";

type Shared = {
  id: string;
  title?: string;
  emphasis: Emphasis;
  /** Blocks that belong together sit together. */
  group?: string;
  /** Ids this block derives from. */
  feeds?: string[];
  /** Source keys, drawn as favicon chips. */
  sources?: string[];
  state: BlockState;
  /** What a pending block is about to hold: "Finding stays". */
  pending?: string;
};

export type EntityFacet =
  | "place"
  | "stay"
  | "flight"
  | "product"
  | "person"
  | "company"
  | "event"
  | "repo"
  | "pull_request"
  | "message"
  | "job";

export type Picture = { profile: string; digest: string };
export type Fact = { label: string; value: string; sources?: string[] };
export type Entity = {
  /** Merge key: one name, one entity, across the run's artifacts. */
  key: string;
  /** The canvas element that holds it, for choose and ask. */
  element?: string;
  facet: EntityFacet;
  name: string;
  descriptor?: string;
  image?: Picture;
  homepage?: string;
  price?: string;
  time?: string;
  facts: Fact[];
  sources?: string[];
  chosen?: boolean;
};

export type ColumnType = "text" | "number" | "money" | "date" | "link" | "long";
export type TableColumn = { label: string; type: ColumnType };
export type TimelineStop = { when: string; title: string; detail?: string };

type Matrix = Extract<ArtifactContent, { kind: "matrix" }>;
type Plain = Extract<ArtifactContent, { kind: "comparison" }>;
type Chart = Extract<ArtifactContent, { kind: "chart" }>;
type Diagram = Extract<ArtifactContent, { kind: "diagram" }>;

export type ProseBlock = Shared & {
  kind: "prose";
  blocks: readonly DocumentNodeView[];
  /** Source keys whose chips close a block ("2") or a list item ("3.1"). */
  cites: Readonly<Record<string, readonly string[]>>;
};
export type StatBlock = Shared & {
  kind: "stat";
  label: string;
  value: string;
  unit?: string;
  basis?: string;
};
export type CalloutBlock = Shared & {
  kind: "callout";
  tone: "decision" | "caution" | "quote";
  text: string;
};
type EntityBlock = Shared & { kind: "entity"; entity: Entity };
export type GalleryBlock = Shared & {
  kind: "gallery";
  facet: EntityFacet;
  entities: Entity[];
  /** The comparison the entities came from: the gallery opens onto it. */
  compare?: Matrix;
};
export type ComparisonBlock = Shared & { kind: "comparison"; content: Matrix | Plain };
export type TableBlock = Shared & {
  kind: "table";
  columns: TableColumn[];
  rows: readonly (readonly string[])[];
};
export type ChartBlock = Shared & {
  kind: "chart";
  chart: Chart;
  /** The sum or the span the chart carries, said above it. */
  headline?: { label: string; value: string };
  /** The table that holds the same rows: the chart's exact values, opened in place. */
  values?: { columns: TableColumn[]; rows: readonly (readonly string[])[] };
};
export type TimelineBlock = Shared & { kind: "timeline"; stops: TimelineStop[] };
export type DiagramBlock = Shared & { kind: "diagram"; diagram: Diagram };
export type ChecklistBlock = Shared & {
  kind: "checklist";
  items: readonly { text: string; completed: boolean }[];
};
export type CodeBlock = Shared & {
  kind: "code";
  language: string;
  text: string;
  notes: readonly { from: number; to: number; text: string }[];
};
export type DocumentBlock = Shared & {
  kind: "document";
  content: Extract<ArtifactContent, { kind: "document" }>;
};

export type Block =
  | ProseBlock
  | StatBlock
  | CalloutBlock
  | EntityBlock
  | GalleryBlock
  | ComparisonBlock
  | TableBlock
  | ChartBlock
  | TimelineBlock
  | DiagramBlock
  | ChecklistBlock
  | CodeBlock
  | DocumentBlock;
export type BlockKind = Block["kind"];

export type Board = {
  id: string;
  title: string;
  lead: string;
  /** A short answer's rest, read on from the lead under the title rather than as a block. */
  more?: ProseBlock;
  blocks: Block[];
  /** Every source a block names, by key. */
  sources: Readonly<Record<string, EvidenceReference>>;
};

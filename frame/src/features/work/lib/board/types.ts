import type { IconRef } from "$shared/ipc/bindings";
import type {
  ArtifactContent,
  DocumentNodeView,
  EvidenceReference,
} from "$shared/ui/data/Artifact";
import type { ChartSpec } from "$shared/ui/data/Chart";

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

// The objects (spec §2). One view type per object, filled by the canvas's adapters from new
// and legacy artifacts; every renderer takes one of these and draws it the same at every zoom.
// Plain display values: no wire types, pictures already resolved to an address the frame may load.

/** An admitted picture: an address the frame may load, and its natural size when known. */
type PictureView = { src: string; width?: number; height?: number };
/** A picture or a mark stands for a thing; with neither, the layout goes without. */
type Likeness = {
  /** The thing's own image: a home's photo, a product shot, a video's poster. */
  picture?: PictureView;
  /** A bare host whose known logo stands for it: `airbnb.com`, `lot.com`. */
  logo?: string;
};

type ObjectBase = {
  /** The canvas element that holds it: choose, ask and open act on it. */
  id: string;
  /** What the object is called where it has a place for a name. */
  title?: string;
  /** Pending while its part still works; the renderer draws its quiet placeholder. */
  state?: "pending" | "ready";
  /** It revises an earlier object: "Updated · 14:20" opens its history. */
  updated?: string;
  /** Sources its facts name by key, drawn as favicon chips on hover. */
  sources?: Readonly<Record<string, EvidenceReference>>;
};

type FigureView = { label: string; value: string; note?: string };
export type ReplyView = ObjectBase & {
  kind: "reply";
  headline: string;
  /** ≤ 480 chars; inline **bold** and `code` only. */
  text: string;
  figures: readonly FigureView[];
  points: readonly string[];
  /** A legacy answer's remaining blocks, read in the opened view. */
  more?: readonly DocumentNodeView[];
};

type PicksFacet =
  | "stay"
  | "flight"
  | "product"
  | "place"
  | "restaurant"
  | "job"
  | "course"
  | "video"
  | "repo"
  | "service"
  | "company"
  | "person"
  | "event"
  | "article"
  | "other";
type FactKind = "text" | "yes" | "no" | "partial" | "rating";
type PickFact = { label: string; value: string; kind: FactKind; sources?: string[] };
type PriceView = { display: string; amount?: number; currency?: string };
type RouteView = {
  from: string;
  to: string;
  /** Clock times as the source wrote them: "07:40". */
  depart?: string;
  arrive?: string;
  duration?: string;
  stops: number;
  /** Where it stops, when known: "FRA". */
  via?: readonly string[];
  carrier?: string;
  carrierHost?: string;
};
export type PickView = Likeness & {
  /** Its canvas element, when it stands as one: choose and ask act on it. */
  element?: string;
  name: string;
  subtitle?: string;
  url?: string;
  price?: PriceView;
  facts: readonly PickFact[];
  rating?: { value: number; max: 5 | 10; count?: number };
  why?: string;
  tags: readonly string[];
  recommended: boolean;
  chosen?: boolean;
  route?: RouteView;
  when?: string;
  /** A video's or a course's length: "12:04", "10 weeks". */
  duration?: string;
  sources?: readonly string[];
};
export type PicksView = ObjectBase & {
  kind: "picks";
  facet: PicksFacet;
  items: readonly PickView[];
};

type StepKind = "travel" | "stay" | "event" | "task" | "milestone" | "note";
/** A pick a step stands on, drawn as a small thumbnail beside it. */
type PickLink = Likeness & { name: string; element?: string };
export type PlanStepView = {
  when?: string;
  title: string;
  detail?: string;
  kind: StepKind;
  cost?: string;
  place?: string;
  pick?: PickLink;
  done?: boolean;
};
export type PlanView = ObjectBase & {
  kind: "plan";
  steps: readonly PlanStepView[];
  total?: { label: string; value: string };
  checkable: boolean;
};

type ListStyle = "todo" | "messages" | "reading" | "requirements";
type ListFrom = {
  /** A site or an app's host for its mark: `slack.com`. */
  host?: string;
  app?: string;
  who?: string;
  when?: string;
  quote?: string;
  url?: string;
};
type ListItemView = {
  title: string;
  detail?: string;
  due?: string;
  priority?: "high";
  from?: ListFrom;
  done?: boolean;
};
export type ListView = ObjectBase & {
  kind: "list";
  style: ListStyle;
  items: readonly ListItemView[];
};

type SheetColumnKind =
  | "text"
  | "number"
  | "money"
  | "percent"
  | "date"
  | "duration"
  | "yes_no"
  | "rating"
  | "link"
  | "entity"
  | "tag";
export type SheetColumn = {
  label: string;
  kind: SheetColumnKind;
  unit?: string;
  currency?: string;
  best?: "max" | "min";
};
export type SheetRow = {
  /** One string per column, typed by it: yes_no ∈ yes|no|partial|unknown, rating "4/5", decimals. */
  cells: readonly string[];
  /** The row's subject, drawn in its entity cell. */
  entity?: Likeness;
  sources?: readonly string[];
};
export type SheetView = ObjectBase & {
  kind: "sheet";
  columns: readonly SheetColumn[];
  rows: readonly SheetRow[];
  note?: string;
};

export type PlotView = ObjectBase & {
  kind: "plot";
  spec: ChartSpec;
};

export type DiagramView = ObjectBase & { kind: "diagram"; diagram: Diagram };

export type CodeView = ObjectBase & {
  kind: "code";
  language: string;
  text: string;
  notes: readonly { from: number; to: number; text: string }[];
  path?: string;
  /** The first line's number in its file. */
  start?: number;
};

type DiffLine = { op: "ctx" | "add" | "del"; text: string };
type DiffHunk = { oldStart: number; newStart: number; lines: readonly DiffLine[] };
export type DiffView = ObjectBase & {
  kind: "diff";
  path: string;
  language: string;
  summary: string;
  hunks: readonly DiffHunk[];
};

export type DocumentObjectView = ObjectBase & {
  kind: "document";
  content: Extract<ArtifactContent, { kind: "document" }>;
};

type DraftDestination = "slack" | "email" | "linkedin" | "x" | "github" | "message";
export type DraftView = ObjectBase & {
  kind: "draft";
  destination: DraftDestination;
  /** A channel, a person, an address, a repository and issue. */
  to?: string;
  subject?: string;
  /** Markdown subset, ≤ 4000 chars. */
  body: string;
  targetUrl?: string;
  /** Who it goes out as, for the avatar row; the person's own name and picture. */
  author?: Likeness & { name: string; handle?: string };
  /** Where Send stands: until Confirm is wired, a draft is only ever a draft. */
  send?: "draft" | "confirming" | "sent";
};

export type MediaView = ObjectBase & {
  kind: "media";
  media: "image" | "video" | "audio";
  /** The image itself, the video's page or the audio file. */
  url: string;
  /** An image's admitted address; the url is where it came from. */
  picture?: PictureView;
  provider?: "youtube" | "vimeo" | "file";
  poster?: PictureView;
  duration?: string;
  startSecs?: number;
};

export type PageObjectView = ObjectBase & {
  kind: "page";
  url: string;
  title: string;
  /** Its captured frame; none when the capture failed. */
  frame?: string;
  icon?: IconRef | null;
  /** The agent is on it right now. */
  live?: boolean;
};

export type NoteView = ObjectBase & {
  kind: "note";
  /** The note's Markdown, as its file holds it. */
  markdown: string;
};

type FileKind = "image" | "pdf" | "text" | "code" | "other";
export type FileView = ObjectBase & {
  kind: "file";
  name: string;
  path?: string;
  file: FileKind;
  /** "PDF document", "Keynote presentation": the system's own words for it. */
  kindLabel?: string;
  size?: number;
  /** An image file, a PDF's first page, or the system icon of anything else. */
  picture?: PictureView;
  /** A text or code file's first lines. */
  lines?: string;
  language?: string;
};

type FolderEntry = { name: string; folder: boolean; picture?: PictureView };
export type FolderView = ObjectBase & {
  kind: "folder";
  name: string;
  path?: string;
  count: number;
  entries: readonly FolderEntry[];
};

/** A member of a project's stack, from its manifests: a known product's host draws its mark. */
type ProjectStackItem = { name: string; host?: string; version?: string; role?: string };
/** An entry of a project's structure; a folder holds its first entries and counts the rest. */
export type ProjectEntry = {
  name: string;
  folder: boolean;
  children?: readonly ProjectEntry[];
  /** Entries the folder holds beyond those listed. */
  more?: number;
};
/** A command the project defines: `dev` is `vite dev`, from `package.json`. */
type ProjectScript = { name: string; command: string; source?: string };
export type ProjectView = ObjectBase & {
  kind: "project";
  name: string;
  /** One line: what the project is. */
  summary: string;
  /** The folder, as an absolute path. */
  root: string;
  stack: readonly ProjectStackItem[];
  /** The root's entries, depth ≤ 3. */
  tree: readonly ProjectEntry[];
  /** Root entries beyond those listed. */
  more?: number;
  scripts: readonly ProjectScript[];
  /** Absent outside a repository. */
  git?: { branch?: string; changed: number; ahead?: number; behind?: number };
};

export type ObjectView =
  | ProjectView
  | ReplyView
  | PicksView
  | PlanView
  | ListView
  | SheetView
  | PlotView
  | DiagramView
  | CodeView
  | DiffView
  | DocumentObjectView
  | DraftView
  | MediaView
  | PageObjectView
  | NoteView
  | FileView
  | FolderView;
export type ObjectKind = ObjectView["kind"];

/** What an object asks of the canvas that holds it; every member optional, absent does nothing. */
export type ObjectActions = {
  /** Opens the object, or one of its members, in the centre. */
  open?: (id: string, member?: number) => void;
  /** Chooses a pick, or takes the choice back. */
  choose?: (element: string, chosen: boolean) => void;
  /** Picks laid side by side as a sheet. */
  compare?: (id: string) => void;
  /** The ask bar takes a question about this. */
  ask?: (subject: string) => void;
  /** A source chip opens its page at the passage. */
  evidence?: (reference: EvidenceReference) => void;
  /** A link, a pick's page or a message's origin, opened over the canvas. */
  link?: (url: string) => void;
  /** A checkable step or item ticked or unticked. */
  check?: (id: string, index: number, done: boolean) => void;
  /** A note's new Markdown, written through the notes store. */
  write?: (id: string, markdown: string) => void;
  /** A draft's Send: always through Confirm. */
  send?: (id: string) => void;
};

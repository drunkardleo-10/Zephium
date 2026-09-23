import type {
  WorkArtifactV1,
  WorkEnvironmentElement,
  WorkEnvironmentReference,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { isAgentExecution } from "./agent-steps";
import type { CanvasPosition, CanvasSize } from "./canvas-model";
import { listingArtifacts, recordArtifacts, subjectKey, subjectsOf } from "./subjects";

type Placement = CanvasPosition & CanvasSize;
export type OrganizePlan = {
  execution: string;
  adds: { reference: WorkEnvironmentReference; placement: Placement }[];
  relations: {
    from: WorkEnvironmentReference;
    to: WorkEnvironmentReference;
    kind: "supports" | "uses";
  }[];
  areaTitle: string;
};

const same = (a: WorkEnvironmentReference, b: WorkEnvironmentReference) =>
  JSON.stringify(a) === JSON.stringify(b);
const FINDINGS_PER_ARTIFACT = 8;
const FINDINGS_PER_RUN = 32;
const SUBJECTS_PER_RUN = 12;
/** Rust's MAX_ENVIRONMENT_ELEMENTS: an add past it is refused, so none is planned. */
const CANVAS_ELEMENTS = 500;
const SIZES = {
  subject: { width: 240, height: 136 },
  pictured: { width: 240, height: 256 },
  finding: { width: 300, height: 140 },
  page: { width: 320, height: 236 },
} as const;
const GAP = 24;
/** The air between two cards stacked in the same column. */
export const CARD_GAP = GAP;
/** The air between two columns of a stage. */
const COLUMN_GAP = 48;
/** The request card every stage opens with; the columns follow it rightwards. */
export const REQUEST_SIZE = { width: 320, height: 150 } as const;
/** One Sources card stands where a stage's cited pages were found. */
export const SOURCES_SIZE = { width: 300, height: 208 } as const;
/** One page card, sized for the frame it keeps. */
export const PAGE_SIZE = SIZES.page;
/** A stage reads left to right: request, Sources, pages, subjects, result. */
export const COLUMNS = {
  sources: (x: number) => x + REQUEST_SIZE.width + COLUMN_GAP,
  pages: (x: number) => COLUMNS.sources(x) + SOURCES_SIZE.width + COLUMN_GAP,
  subjects: (x: number) => COLUMNS.pages(x) + SIZES.page.width + COLUMN_GAP,
  objects: (x: number) => COLUMNS.subjects(x) + SIZES.pictured.width + COLUMN_GAP,
} as const;

function roots(execution: WorkExecutionFact): WorkArtifactV1[] {
  return execution.artifacts.filter((artifact) =>
    execution.spec.nodes.some((node) => node.node === artifact.node && node.parent === null),
  );
}
/** Whether an artifact yields at least one canvas object under the agent projection. */
function placeable(artifact: WorkArtifactV1): boolean {
  switch (artifact.data.kind) {
    // Cited pages are rows on the run's Sources card, not elements of their own.
    case "evidence_collection":
      return (artifact.data.subjects?.length ?? 0) > 0;
    case "findings":
      return artifact.data.items.length > 0 || (artifact.data.subjects?.length ?? 0) > 0;
    default:
      return true;
  }
}
/** Subject hubs already on the canvas for this run, by merge key. */
function placedSubjects(
  snapshot: WorkEnvironmentSnapshot,
  execution: WorkExecutionFact,
): Map<string, WorkEnvironmentReference> {
  const keys = new Map<string, WorkEnvironmentReference>();
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (reference.kind !== "subject" || reference.execution !== execution.id) continue;
    const artifact = execution.artifacts.find((artifact) => artifact.id === reference.artifact);
    const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
    if (subject) keys.set(subjectKey(subject), reference);
  }
  return keys;
}
function unplacedRoots(
  snapshot: WorkEnvironmentSnapshot,
  execution: WorkExecutionFact,
): WorkArtifactV1[] {
  const placed = referenced(snapshot, execution.id);
  const records = recordArtifacts(execution);
  const listings = listingArtifacts(execution);
  const hubs = placedSubjects(snapshot, execution);
  return roots(execution).filter((artifact) => {
    if (placed.has(artifact.id) || !placeable(artifact)) return false;
    // A catalogue read establishes nothing on its own; its rows only enrich.
    if (listings.has(artifact.id)) return false;
    if (!records.has(artifact.id)) return true;
    return (
      hubs.size < SUBJECTS_PER_RUN &&
      subjectsOf(artifact).some((subject) => !hubs.has(subjectKey(subject)))
    );
  });
}
function referenced(snapshot: WorkEnvironmentSnapshot, execution: string): Set<string> {
  const ids = new Set<string>();
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (
      (reference.kind === "artifact" ||
        reference.kind === "subject" ||
        reference.kind === "finding") &&
      reference.execution === execution
    )
      ids.add(reference.artifact);
  }
  return ids;
}

/** Latest execution whose root outputs are not yet on the canvas. Agent runs
 * organize while running; reviewed plans organize once they settle. */
export function pendingOrganize(
  snapshot: WorkEnvironmentSnapshot,
  projection: WorkRuntimeProjection,
): WorkExecutionFact | null {
  const execution = projection.executions.at(-1);
  if (!execution || projection.interrupted.includes(execution.id)) return null;
  if (snapshot.elements.length >= CANVAS_ELEMENTS) return null;
  const agent = isAgentExecution(execution);
  if (!agent && !["completed", "needs_review"].includes(execution.status)) return null;
  if (agent) return unplacedRoots(snapshot, execution).length ? execution : null;
  const placed = referenced(snapshot, execution.id);
  return roots(execution).some((artifact) => !placed.has(artifact.id)) ? execution : null;
}

/** Deterministic first placement around the objective; user arrangement is never rewritten. */
export function organizeExecution(
  projection: WorkRuntimeProjection,
  execution: WorkExecutionFact,
  anchor: CanvasPosition,
  snapshot?: WorkEnvironmentSnapshot,
): OrganizePlan {
  const plan =
    snapshot && isAgentExecution(execution)
      ? organizeAgentRun(projection, execution, anchor, snapshot)
      : organizeReviewedRun(projection, execution, anchor);
  // A long work fills the canvas; what does not fit stays in its run's result.
  const room = CANVAS_ELEMENTS - (snapshot?.elements.length ?? 0);
  return plan.adds.length > room ? { ...plan, adds: plan.adds.slice(0, Math.max(room, 0)) } : plan;
}

function organizeReviewedRun(
  projection: WorkRuntimeProjection,
  execution: WorkExecutionFact,
  anchor: CanvasPosition,
): OrganizePlan {
  const objective = projection.work.id;
  const artifacts = roots(execution);
  const adds: OrganizePlan["adds"] = [];
  const relations: OrganizePlan["relations"] = [];
  const ref = (artifact: string): WorkEnvironmentReference => ({
    kind: "artifact",
    objective,
    execution: execution.id,
    artifact,
  });
  const matrix = artifacts.find((artifact) => artifact.data.kind === "comparison_matrix");
  const findings = artifacts.filter((artifact) => artifact.data.kind === "findings");
  const sources = artifacts.filter((artifact) => artifact.data.kind === "evidence_collection");
  const subjectOwner =
    matrix ??
    findings.find((artifact) => (artifact.data.subjects?.length ?? 0) > 0) ??
    sources.find((artifact) => (artifact.data.subjects?.length ?? 0) > 0);
  const subjects = subjectOwner ? (subjectOwner.data.subjects ?? []) : [];
  // A reviewed run stands beside its request, not under it.
  let y = anchor.y;
  const x0 = COLUMNS.sources(anchor.x);
  const subjectRefs: WorkEnvironmentReference[] = [];
  if (subjects.length) {
    subjects.forEach((_, index) => {
      const reference: WorkEnvironmentReference = {
        kind: "subject",
        objective,
        execution: execution.id,
        artifact: subjectOwner!.id,
        index,
      };
      subjectRefs.push(reference);
      adds.push({
        reference,
        placement: { x: x0 + index * (SIZES.subject.width + GAP), y, ...SIZES.subject },
      });
    });
    y += SIZES.subject.height + GAP * 2;
  }
  const columnX = [x0, x0 + 700];
  let leftY = y;
  let rightY = y;
  if (matrix) {
    const width = Math.min(1100, Math.max(640, 200 + subjects.length * 180));
    adds.push({ reference: ref(matrix.id), placement: { x: x0, y: leftY, width, height: 380 } });
    for (const subject of subjectRefs)
      relations.push({ from: subject, to: ref(matrix.id), kind: "uses" });
    leftY += 380 + GAP;
    columnX[1] = x0 + width + GAP;
    rightY = y;
  }
  for (const artifact of findings) {
    adds.push({
      reference: ref(artifact.id),
      placement: { x: columnX[1]!, y: rightY, width: 420, height: 360 },
    });
    rightY += 360 + GAP;
  }
  for (const artifact of sources) {
    adds.push({
      reference: ref(artifact.id),
      placement: { x: columnX[1]!, y: rightY, width: 360, height: 300 },
    });
    for (const finding of findings)
      relations.push({ from: ref(artifact.id), to: ref(finding.id), kind: "supports" });
    if (matrix) relations.push({ from: ref(artifact.id), to: ref(matrix.id), kind: "supports" });
    rightY += 300 + GAP;
  }
  for (const artifact of artifacts) {
    if (artifact === matrix || findings.includes(artifact) || sources.includes(artifact)) continue;
    adds.push({
      reference: ref(artifact.id),
      placement: { x: x0, y: leftY, ...objectSize(artifact) },
    });
    leftY += objectSize(artifact).height + GAP;
  }
  const areaTitle = projection.work.objective.slice(0, 64);
  return { execution: execution.id, adds, relations, areaTitle };
}

function objectSize(artifact: WorkArtifactV1): CanvasSize {
  switch (artifact.data.kind) {
    case "document":
      return { width: 480, height: 360 };
    case "table":
      return { width: 560, height: 320 };
    case "comparison_matrix":
      return { width: 560, height: 300 };
    default:
      return { width: 420, height: 300 };
  }
}

/** Agent runs land incrementally: every new root artifact becomes objects that
 * join what is already there. Sources connect to the findings they support,
 * findings to their subjects, subjects to the comparison they appear in. */
function organizeAgentRun(
  projection: WorkRuntimeProjection,
  execution: WorkExecutionFact,
  anchor: CanvasPosition,
  snapshot: WorkEnvironmentSnapshot,
): OrganizePlan {
  const objective = projection.work.id;
  const fresh = unplacedRoots(snapshot, execution);
  const records = recordArtifacts(execution);
  const adds: OrganizePlan["adds"] = [];
  const relations: OrganizePlan["relations"] = [];
  const existing = snapshot.elements.filter(
    (element) => "execution" in element.reference && element.reference.execution === execution.id,
  );
  const placementOf = (element: WorkEnvironmentElement) =>
    snapshot.view.placements.find((place) => place.element === element.id);
  const bottom = (kind: WorkEnvironmentReference["kind"], fallback: number) =>
    Math.max(
      fallback,
      ...existing
        .filter((element) => element.reference.kind === kind)
        .flatMap((element) => {
          const place = placementOf(element);
          return place ? [place.y + place.height + GAP] : [];
        }),
    );
  const count = (kind: WorkEnvironmentReference["kind"]) =>
    existing.filter((element) => element.reference.kind === kind).length;
  // One flow, left to right: the Sources cards, the pages read from them, the
  // subjects they establish, then findings and the published objects.
  const subjectsX = COLUMNS.subjects(anchor.x);
  const findingsX = COLUMNS.objects(anchor.x);
  const objectsX = findingsX;
  let subjectCount = count("subject");
  let subjectY = bottom("subject", anchor.y);
  let findingY = bottom("finding", anchor.y);
  let objectY = Math.max(bottom("artifact", anchor.y), bottom("finding", anchor.y));

  // Subjects are hubs: one per name across the run.
  const subjectByName = placedSubjects(snapshot, execution);
  const subjectRef = (artifact: WorkArtifactV1, index: number): WorkEnvironmentReference => ({
    kind: "subject",
    objective,
    execution: execution.id,
    artifact: artifact.id,
    index,
  });
  const artifactRef = (artifact: WorkArtifactV1): WorkEnvironmentReference => ({
    kind: "artifact",
    objective,
    execution: execution.id,
    artifact: artifact.id,
  });
  const admitSubjects = (artifact: WorkArtifactV1): WorkEnvironmentReference[] => {
    return subjectsOf(artifact).map((subject, index) => {
      const name = subjectKey(subject);
      const known = subjectByName.get(name);
      if (known) return known;
      const reference = subjectRef(artifact, index);
      if (subjectCount < SUBJECTS_PER_RUN) {
        const size = subject.image_candidates?.length ? SIZES.pictured : SIZES.subject;
        adds.push({ reference, placement: { x: subjectsX, y: subjectY, ...size } });
        subjectY += size.height + GAP;
        subjectCount += 1;
        subjectByName.set(name, reference);
      }
      return reference;
    });
  };
  for (const artifact of fresh) {
    const subjects = admitSubjects(artifact);
    if (records.has(artifact.id)) continue;
    if (artifact.data.kind === "findings") {
      let findingCount = count("finding");
      for (const [index, item] of artifact.data.items.slice(0, FINDINGS_PER_ARTIFACT).entries()) {
        if (findingCount >= FINDINGS_PER_RUN) break;
        const reference: WorkEnvironmentReference = {
          kind: "finding",
          objective,
          execution: execution.id,
          artifact: artifact.id,
          index,
        };
        adds.push({
          reference,
          placement: { x: findingsX, y: findingY, ...SIZES.finding },
        });
        findingY += SIZES.finding.height + GAP;
        findingCount += 1;
        const subject =
          item.subject === null || item.subject === undefined ? undefined : subjects[item.subject];
        if (subject) relations.push({ from: reference, to: subject, kind: "supports" });
      }
      continue;
    }
    const reference = artifactRef(artifact);
    const size = objectSize(artifact);
    adds.push({ reference, placement: { x: objectsX, y: objectY, ...size } });
    objectY += size.height + GAP;
    if (artifact.data.kind === "comparison_matrix")
      for (const subject of subjects)
        relations.push({ from: subject, to: reference, kind: "uses" });
  }
  return {
    execution: execution.id,
    adds,
    relations,
    areaTitle: projection.work.objective.slice(0, 64),
  };
}

export function elementFor(
  snapshot: WorkEnvironmentSnapshot,
  reference: WorkEnvironmentReference,
): WorkEnvironmentElement | undefined {
  return snapshot.elements.find((element) => same(element.reference, reference));
}

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
import { environmentStages, type WorkStage } from "./project-environment-thread";
import { SIZES, stageLayout, type ClusterKind, type StageContents } from "./stage-layout";

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
const SUBJECTS_PER_RUN = 12;
/** Rust's MAX_ENVIRONMENT_ELEMENTS: an add past it is refused, so none is planned. */
const CANVAS_ELEMENTS = 500;
const GAP = 24;

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
  /** The stage the run serves, as the canvas draws it; derived when absent. */
  stage?: WorkStage,
): OrganizePlan {
  const plan =
    snapshot && isAgentExecution(execution)
      ? organizeAgentRun(projection, execution, snapshot, stage)
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
  const x0 = anchor.x + SIZES.request.width + 48;
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
    case "findings":
      return SIZES.findings;
    case "document":
      return SIZES.document;
    case "comparison_matrix":
      return SIZES.comparison;
    default:
      return SIZES.result;
  }
}

/** Agent runs land incrementally: every new root artifact becomes objects that
 * join what is already there, each in the slot its stage's layout gives it.
 * Subjects are hubs, one per name; a findings artifact is one card that
 * supports the subjects it names; a comparison uses its subjects. */
function organizeAgentRun(
  projection: WorkRuntimeProjection,
  execution: WorkExecutionFact,
  snapshot: WorkEnvironmentSnapshot,
  given?: WorkStage,
): OrganizePlan {
  const objective = projection.work.id;
  const stage =
    given ??
    environmentStages(snapshot, new Map([[objective, projection]])).find((candidate) =>
      candidate.executions.includes(execution.id),
    );
  const fresh = unplacedRoots(snapshot, execution);
  const records = recordArtifacts(execution);
  const relations: OrganizePlan["relations"] = [];
  const pending: { reference: WorkEnvironmentReference; size: CanvasSize }[] = [];
  const contents: StageContents = { ...stage?.contents };
  const join = (kind: ClusterKind, reference: WorkEnvironmentReference, size: CanvasSize) => {
    pending.push({ reference, size });
    const cluster = contents[kind];
    contents[kind] = {
      ...cluster,
      members: [...(cluster?.members ?? []), { id: JSON.stringify(reference), size }],
    };
  };
  let subjectCount = snapshot.elements.filter(
    (element) =>
      element.reference.kind === "subject" && element.reference.execution === execution.id,
  ).length;
  const subjectByName = placedSubjects(snapshot, execution);
  const admitSubjects = (artifact: WorkArtifactV1): WorkEnvironmentReference[] =>
    subjectsOf(artifact).map((subject, index) => {
      const name = subjectKey(subject);
      const known = subjectByName.get(name);
      if (known) return known;
      const reference: WorkEnvironmentReference = {
        kind: "subject",
        objective,
        execution: execution.id,
        artifact: artifact.id,
        index,
      };
      if (subjectCount < SUBJECTS_PER_RUN) {
        join(
          "subjects",
          reference,
          subject.image_candidates?.length ? SIZES.pictured : SIZES.subject,
        );
        subjectCount += 1;
        subjectByName.set(name, reference);
      }
      return reference;
    });
  for (const artifact of fresh) {
    const subjects = admitSubjects(artifact);
    if (records.has(artifact.id)) continue;
    const reference: WorkEnvironmentReference = {
      kind: "artifact",
      objective,
      execution: execution.id,
      artifact: artifact.id,
    };
    if (artifact.data.kind === "findings") {
      join("findings", reference, SIZES.findings);
      const named = new Set<number>();
      for (const item of artifact.data.items)
        if (item.subject !== null && item.subject !== undefined) named.add(item.subject);
      for (const index of named) {
        const subject = subjects[index];
        if (subject) relations.push({ from: reference, to: subject, kind: "supports" });
      }
      continue;
    }
    join("results", reference, objectSize(artifact));
    if (artifact.data.kind === "comparison_matrix")
      for (const subject of subjects)
        relations.push({ from: subject, to: reference, kind: "uses" });
  }
  const place = stage?.place ?? { x: 80, y: 120, ...SIZES.request };
  const { positions } = stageLayout(place, contents);
  // A card past its cluster's cap only counts on the cluster's label.
  const adds = pending.flatMap(({ reference, size }) => {
    const position = positions[JSON.stringify(reference)];
    return position ? [{ reference, placement: { ...position, ...size } }] : [];
  });
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

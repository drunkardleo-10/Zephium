import type {
  WorkArtifactV1,
  WorkEnvironmentElement,
  WorkEnvironmentReference,
  WorkEnvironmentSnapshot,
  WorkEvidenceLink,
  WorkExecutionFact,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { isAgentExecution } from "./agent-steps";
import type { CanvasPosition, CanvasSize } from "./canvas-model";

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
const SOURCES_PER_ARTIFACT = 5;
const SOURCES_PER_RUN = 24;
const FINDINGS_PER_ARTIFACT = 8;
const FINDINGS_PER_RUN = 32;
const SUBJECTS_PER_RUN = 12;
const SIZES = {
  subject: { width: 240, height: 112 },
  finding: { width: 300, height: 140 },
  source: { width: 260, height: 84 },
} as const;
const GAP = 24;

function roots(execution: WorkExecutionFact): WorkArtifactV1[] {
  return execution.artifacts.filter((artifact) =>
    execution.spec.nodes.some((node) => node.node === artifact.node && node.parent === null),
  );
}
/** Whether an artifact yields at least one canvas object under the agent projection. */
function placeable(artifact: WorkArtifactV1): boolean {
  switch (artifact.data.kind) {
    case "evidence_collection":
      return (artifact.data.entries?.length ?? 0) > 0;
    case "findings":
      return artifact.data.items.length > 0 || (artifact.data.subjects?.length ?? 0) > 0;
    default:
      return true;
  }
}
function referenced(snapshot: WorkEnvironmentSnapshot, execution: string): Set<string> {
  const ids = new Set<string>();
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (
      (reference.kind === "artifact" ||
        reference.kind === "subject" ||
        reference.kind === "finding" ||
        reference.kind === "source") &&
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
  const agent = isAgentExecution(execution);
  if (!agent && !["completed", "needs_review"].includes(execution.status)) return null;
  const placed = referenced(snapshot, execution.id);
  const unplaced = roots(execution).filter(
    (artifact) => !placed.has(artifact.id) && (!agent || placeable(artifact)),
  );
  return unplaced.length ? execution : null;
}

/** Deterministic first placement around the objective; user arrangement is never rewritten. */
export function organizeExecution(
  projection: WorkRuntimeProjection,
  execution: WorkExecutionFact,
  anchor: CanvasPosition,
  snapshot?: WorkEnvironmentSnapshot,
): OrganizePlan {
  if (snapshot && isAgentExecution(execution))
    return organizeAgentRun(projection, execution, anchor, snapshot);
  return organizeReviewedRun(projection, execution, anchor);
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
  let y = anchor.y;
  const x0 = anchor.x;
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
      return { width: 760, height: 380 };
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
  const placed = referenced(snapshot, execution.id);
  const fresh = roots(execution).filter(
    (artifact) => !placed.has(artifact.id) && placeable(artifact),
  );
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
  const subjectsY = anchor.y;
  const columnsY = anchor.y + SIZES.subject.height + GAP * 2;
  const findingsX = anchor.x;
  const sourcesX = anchor.x + SIZES.finding.width + GAP * 2;
  const objectsX = sourcesX + SIZES.source.width * 2 + GAP * 3;
  let subjectCount = count("subject");
  let findingY = bottom("finding", columnsY);
  let sourceCount = count("source");
  let sourceY = bottom("source", columnsY) - (sourceCount % 2 ? SIZES.source.height + GAP : 0);
  let objectY = bottom("artifact", columnsY);

  // Subjects are hubs: one per name across the run.
  const subjectByName = new Map<string, WorkEnvironmentReference>();
  for (const element of existing) {
    const reference = element.reference;
    if (reference.kind !== "subject") continue;
    const artifact = execution.artifacts.find((artifact) => artifact.id === reference.artifact);
    const subjects =
      artifact &&
      (artifact.data.kind === "comparison_matrix" ||
        artifact.data.kind === "findings" ||
        artifact.data.kind === "evidence_collection")
        ? artifact.data.subjects
        : undefined;
    const name = subjects?.[reference.index]?.name.trim().toLowerCase();
    if (name) subjectByName.set(name, reference);
  }
  // Sources are matched to findings by the exact evidence link they cite.
  const sourceByLink = new Map<string, WorkEnvironmentReference>();
  const linkKey = (link: WorkEvidenceLink) => `${link.extraction_id}:${link.source_id}`;
  for (const element of existing) {
    const reference = element.reference;
    if (reference.kind !== "source") continue;
    const artifact = execution.artifacts.find((artifact) => artifact.id === reference.artifact);
    const entry =
      artifact?.data.kind === "evidence_collection"
        ? artifact.data.entries?.[reference.index]
        : undefined;
    const link = entry ? artifact!.evidence[entry.evidence] : undefined;
    if (link) sourceByLink.set(linkKey(link), reference);
  }
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
    const subjects =
      artifact.data.kind === "comparison_matrix" ||
      artifact.data.kind === "findings" ||
      artifact.data.kind === "evidence_collection"
        ? (artifact.data.subjects ?? [])
        : [];
    return subjects.map((subject, index) => {
      const name = subject.name.trim().toLowerCase();
      const known = subjectByName.get(name);
      if (known) return known;
      const reference = subjectRef(artifact, index);
      if (subjectCount < SUBJECTS_PER_RUN) {
        adds.push({
          reference,
          placement: {
            x: anchor.x + subjectCount * (SIZES.subject.width + GAP),
            y: subjectsY,
            ...SIZES.subject,
          },
        });
        subjectCount += 1;
        subjectByName.set(name, reference);
      }
      return reference;
    });
  };
  for (const artifact of fresh) {
    if (artifact.data.kind === "evidence_collection") {
      const entries = artifact.data.entries ?? [];
      for (const [index, entry] of entries.slice(0, SOURCES_PER_ARTIFACT).entries()) {
        if (sourceCount >= SOURCES_PER_RUN) break;
        const link = artifact.evidence[entry.evidence];
        if (!link) continue;
        const reference: WorkEnvironmentReference = {
          kind: "source",
          objective,
          execution: execution.id,
          artifact: artifact.id,
          index,
        };
        adds.push({
          reference,
          placement: {
            x: sourcesX + (sourceCount % 2) * (SIZES.source.width + GAP),
            y: sourceY,
            ...SIZES.source,
          },
        });
        sourceByLink.set(linkKey(link), reference);
        sourceCount += 1;
        if (sourceCount % 2 === 0) sourceY += SIZES.source.height + GAP;
      }
      continue;
    }
    const subjects = admitSubjects(artifact);
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
        for (const cited of item.evidence ?? []) {
          const link = artifact.evidence[cited];
          const source = link ? sourceByLink.get(linkKey(link)) : undefined;
          if (source) relations.push({ from: source, to: reference, kind: "supports" });
        }
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
    for (const link of artifact.evidence) {
      const source = sourceByLink.get(linkKey(link));
      if (source) relations.push({ from: source, to: reference, kind: "supports" });
    }
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

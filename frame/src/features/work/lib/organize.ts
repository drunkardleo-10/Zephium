import type {
  WorkEnvironmentElement,
  WorkEnvironmentReference,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
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

/** Latest settled execution whose root outputs are not yet on the canvas. */
export function pendingOrganize(
  snapshot: WorkEnvironmentSnapshot,
  projection: WorkRuntimeProjection,
): WorkExecutionFact | null {
  const execution = projection.executions.at(-1);
  if (!execution || !["completed", "needs_review"].includes(execution.status)) return null;
  if (projection.interrupted.includes(execution.id)) return null;
  const attached = snapshot.elements.some(
    (element) =>
      (element.reference.kind === "artifact" ||
        element.reference.kind === "subject" ||
        element.reference.kind === "finding") &&
      element.reference.execution === execution.id,
  );
  return attached ? null : execution;
}

/** Deterministic first placement around the objective; user arrangement is never rewritten. */
export function organizeExecution(
  projection: WorkRuntimeProjection,
  execution: WorkExecutionFact,
  anchor: CanvasPosition,
): OrganizePlan {
  const objective = projection.work.id;
  const roots = execution.artifacts.filter((artifact) =>
    execution.spec.nodes.some((node) => node.node === artifact.node && node.parent === null),
  );
  const adds: OrganizePlan["adds"] = [];
  const relations: OrganizePlan["relations"] = [];
  const ref = (artifact: string): WorkEnvironmentReference => ({
    kind: "artifact",
    objective,
    execution: execution.id,
    artifact,
  });
  const matrix = roots.find((artifact) => artifact.data.kind === "comparison_matrix");
  const findings = roots.filter((artifact) => artifact.data.kind === "findings");
  const sources = roots.filter((artifact) => artifact.data.kind === "evidence_collection");
  const subjectOwner =
    matrix ??
    findings.find((artifact) => (artifact.data.subjects?.length ?? 0) > 0) ??
    sources.find((artifact) => (artifact.data.subjects?.length ?? 0) > 0);
  const subjects = subjectOwner ? (subjectOwner.data.subjects ?? []) : [];
  const gap = 24;
  let y = anchor.y;
  const x0 = anchor.x;
  const subjectRefs: WorkEnvironmentReference[] = [];
  if (subjects.length) {
    const width = 240;
    const height = 112;
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
        placement: { x: x0 + index * (width + gap), y, width, height },
      });
    });
    y += height + gap * 2;
  }
  const columnX = [x0, x0 + 700];
  let leftY = y;
  let rightY = y;
  if (matrix) {
    const width = Math.min(1100, Math.max(640, 200 + subjects.length * 180));
    adds.push({ reference: ref(matrix.id), placement: { x: x0, y: leftY, width, height: 380 } });
    for (const subject of subjectRefs)
      relations.push({ from: subject, to: ref(matrix.id), kind: "uses" });
    leftY += 380 + gap;
    columnX[1] = x0 + width + gap;
    rightY = y;
  }
  for (const artifact of findings) {
    adds.push({
      reference: ref(artifact.id),
      placement: { x: columnX[1]!, y: rightY, width: 420, height: 360 },
    });
    rightY += 360 + gap;
  }
  for (const artifact of sources) {
    adds.push({
      reference: ref(artifact.id),
      placement: { x: columnX[1]!, y: rightY, width: 360, height: 300 },
    });
    for (const finding of findings)
      relations.push({ from: ref(artifact.id), to: ref(finding.id), kind: "supports" });
    if (matrix) relations.push({ from: ref(artifact.id), to: ref(matrix.id), kind: "supports" });
    rightY += 300 + gap;
  }
  for (const artifact of roots) {
    if (artifact === matrix || findings.includes(artifact) || sources.includes(artifact)) continue;
    const size: CanvasSize =
      artifact.data.kind === "document"
        ? { width: 480, height: 360 }
        : artifact.data.kind === "table"
          ? { width: 560, height: 320 }
          : { width: 420, height: 300 };
    adds.push({ reference: ref(artifact.id), placement: { x: x0, y: leftY, ...size } });
    leftY += size.height + gap;
  }
  const areaTitle = projection.work.objective.slice(0, 64);
  return { execution: execution.id, adds, relations, areaTitle };
}

export function elementFor(
  snapshot: WorkEnvironmentSnapshot,
  reference: WorkEnvironmentReference,
): WorkEnvironmentElement | undefined {
  return snapshot.elements.find((element) => same(element.reference, reference));
}

import type {
  WorkRuntimeProjection,
  WorkPlanRevision,
  WorkArtifactV1,
  WorkExecutionFact,
  WorkArtifactDataV1,
  WorkSignalV1,
} from "$domain/work";
import type { ArtifactView, ArtifactContent } from "$shared/ui/data/Artifact";
import * as m from "$shared/i18n/messages";
import type { CanvasItem, CanvasLink } from "./canvas-model";

/** Display derivation of one objective; neither WorkProjectionV1 nor a store. */
export type ProjectedWork = {
  key: string;
  title: string;
  objective: string;
  phase: string;
  notice?: { title: string; detail: string };
  questions: readonly { key: string; prompt: string; options: readonly string[] }[];
  items: readonly (CanvasItem & { artifactKey?: string })[];
  links: readonly CanvasLink[];
  artifacts: readonly ArtifactView[];
  actions: readonly {
    key: string;
    label: string;
    scope: string;
    consequence: string;
    disabledReason?: string;
  }[];
};

function content(data: WorkArtifactDataV1): ArtifactContent {
  switch (data.kind) {
    case "document":
    case "table":
    case "comparison":
    case "checklist":
      return data;
    case "chart":
      return { kind: "chart", xLabel: data.x_label, yLabel: data.y_label, series: data.series };
    case "evidence_collection":
      return { kind: "sources", summary: data.summary };
    case "browser_resource_preview":
      return { kind: "browser", title: data.title, location: data.url, summary: data.summary };
  }
}

export function artifactView(artifact: WorkArtifactV1, execution: WorkExecutionFact): ArtifactView {
  const user = execution.user_artifacts?.find((value) => value.artifact === artifact.id);
  const evidence = user?.edited_data ? user.evidence : artifact.evidence;
  return {
    key: artifact.id,
    title: artifact.title,
    content: content(user?.edited_data ?? artifact.data),
    reviewLabel:
      user?.decision === "accepted"
        ? m.work_accepted()
        : user?.decision === "rejected"
          ? m.work_rejected()
          : artifact.review === "mechanical"
            ? m.work_mechanical_review()
            : m.work_review_required(),
    evidence: evidence.map((link, index) => ({
      key: `${link.extraction_id}:${link.source_id}`,
      label: m.work_source_number({ number: index + 1 }),
    })),
  };
}

/** Display derivation only. A historical execution must join its original plan. */
export function projectWork(
  state: WorkRuntimeProjection,
  plan: WorkPlanRevision | null,
  executionId: string | null,
  activity: readonly WorkSignalV1[] = [],
): ProjectedWork {
  const execution = state.executions.find((item) => item.id === executionId);
  const interrupted = !!execution && state.interrupted.includes(execution.id);
  const exactPlan =
    plan && (!execution || plan.revision === execution.spec.plan_revision) ? plan : null;
  const nodes = exactPlan?.draft.nodes ?? [];
  const artifacts = execution?.artifacts ?? [];
  const live =
    execution &&
    ["approved", "running", "cancel_requested"].includes(execution.status) &&
    !interrupted;
  const nodeIds = new Set(nodes.map((node) => node.id));
  const items: ProjectedWork["items"][number][] = nodes.map((node) => {
    const attempts = execution?.attempts.filter((attempt) => attempt.node === node.id) ?? [];
    const signal = activity.find(
      (entry) => entry.execution === execution?.id && entry.node === node.id,
    );
    const scope = execution?.spec.nodes.find((item) => item.node === node.id);
    return {
      id: node.id,
      title: node.objective.slice(0, 512),
      kind: scope?.parent
        ? m.work_child_agent()
        : scope &&
            ["coordinate", "coordinate_public_discovery", "coordinate_public_research"].includes(
              scope.capability.kind,
            )
          ? m.work_primary_agent()
          : m.work_plan_node(),
      detail: node.outputs
        .map((output) => output.description)
        .join("\n")
        .slice(0, 2048),
      status: interrupted
        ? m.work_interrupted()
        : signal
          ? activityLabel(signal.activity)
          : (attempts.at(-1)?.status ?? m.work_not_started()),
    };
  });
  const links: ProjectedWork["links"][number][] = nodes.flatMap((node) =>
    node.dependencies
      .filter((id) => nodeIds.has(id))
      .map((id) => ({
        id: `${id}:${node.id}`,
        source: id,
        target: node.id,
        kind: "dependency" as const,
      })),
  );
  for (const scope of execution?.spec.nodes ?? []) {
    if (scope.parent && nodeIds.has(scope.parent) && nodeIds.has(scope.node))
      links.push({
        id: `owner:${scope.parent}:${scope.node}`,
        source: scope.parent,
        target: scope.node,
        kind: "reference",
      });
  }
  for (const artifact of artifacts) {
    items.push({
      id: artifact.id,
      title: artifact.title,
      kind: m.work_result(),
      detail: artifact.output,
      status: artifactView(artifact, execution!).reviewLabel,
      artifactKey: artifact.id,
    });
    if (nodeIds.has(artifact.node))
      links.push({
        id: `${artifact.node}:${artifact.id}`,
        source: artifact.node,
        target: artifact.id,
        kind: "reference",
      });
  }
  const actions: ProjectedWork["actions"][number][] = [];
  if (live && execution.status !== "cancel_requested")
    actions.push({
      key: `cancel:${execution.id}`,
      label: m.work_cancel(),
      scope: m.work_cancel_scope(),
      consequence: m.work_cancel_consequence(),
    });
  if (interrupted)
    actions.push({
      key: `acknowledge:${execution.id}`,
      label: m.work_acknowledge(),
      scope: m.work_interrupted(),
      consequence: m.work_acknowledge_consequence(),
    });
  if (!live)
    actions.push({
      key: state.work.lifecycle === "archived" ? "restore" : "archive",
      label: state.work.lifecycle === "archived" ? m.resource_restore() : m.work_archive(),
      scope: state.work.objective.slice(0, 4096),
      consequence: m.work_archive_consequence(),
    });
  return {
    key: `${state.work.profile}:${state.work.id}`,
    title: m.work_workspace(),
    objective: state.work.objective,
    phase: interrupted ? m.work_interrupted() : (execution?.status ?? state.work.status),
    notice: interrupted
      ? { title: m.work_interrupted(), detail: m.work_interruption_detail() }
      : undefined,
    questions: state.work.questions
      .filter((question) => question.state === "active")
      .map((question) => ({
        key: question.id,
        prompt: question.prompt,
        options: question.options,
      })),
    items,
    links,
    artifacts: execution ? artifacts.map((artifact) => artifactView(artifact, execution)) : [],
    actions,
  };
}

function activityLabel(activity: WorkSignalV1["activity"]): string {
  const labels: Record<WorkSignalV1["activity"], string> = {
    planning: m.work_activity_planning(),
    delegating: m.work_activity_delegating(),
    reading: m.work_activity_reading(),
    comparing: m.work_activity_comparing(),
    producing_artifact: m.work_activity_producing(),
    waiting_for_approval: m.work_activity_approval(),
    waiting_for_human: m.work_activity_human(),
    cancelling: m.work_activity_cancelling(),
    finishing: m.work_activity_finishing(),
  };
  return labels[activity];
}

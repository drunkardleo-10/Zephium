import type {
  WorkRuntimeProjection,
  WorkPlanRevision,
  WorkArtifactV1,
  WorkExecutionFact,
  WorkArtifactDataV1,
  WorkSignalV1,
  WorkEvidenceLink,
} from "$domain/work";
import type { ArtifactView, ArtifactContent, EvidenceReference } from "$shared/ui/data/Artifact";
import * as m from "$shared/i18n/messages";
import type { CanvasItem, CanvasLink } from "./canvas-model";
import { fileFolder } from "./work-files";
import { isLive } from "./agent-steps";

/** Hostname for a source label. Resolved here so a small lazy chunk that needs
 * one line of text does not pull the whole Artifact UI module with it. */
function displayHost(value: string | undefined): string {
  if (!value) return "";
  try {
    const url = new URL(value);
    return ["https:", "http:"].includes(url.protocol) ? url.hostname.replace(/^www\./u, "") : "";
  } catch {
    return "";
  }
}

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

type Refs = (indices: readonly number[] | undefined) => EvidenceReference[];
function subjects(
  list:
    | readonly {
        name: string;
        descriptor?: string | null;
        homepage?: string | null;
        image_candidates?: readonly string[] | null;
      }[]
    | undefined,
) {
  return (list ?? []).map((subject) => ({
    name: subject.name,
    ...(subject.descriptor ? { descriptor: subject.descriptor } : {}),
    ...(subject.homepage ? { homepage: subject.homepage } : {}),
    ...(subject.image_candidates?.length
      ? { imageCandidates: subject.image_candidates.slice(0, 3) }
      : {}),
  }));
}
function content(data: WorkArtifactDataV1, refs: Refs): ArtifactContent {
  switch (data.kind) {
    case "document":
      return {
        kind: "document",
        paragraphs: data.paragraphs,
        ...(data.formatted ? { formatted: data.formatted } : {}),
      };
    case "table":
    case "comparison":
    case "checklist":
      return data;
    case "chart":
      return {
        kind: "chart",
        xLabel: data.x_label,
        yLabel: data.y_label,
        series: data.series.map((entry) => ({
          name: entry.name,
          points: entry.points.map((point) => ({
            label: point.label,
            value: point.value,
            ...(point.evidence?.length ? { evidence: refs(point.evidence) } : {}),
          })),
        })),
        ...(data.basis
          ? {
              basis: {
                method: data.basis.method,
                ...(data.basis.conditions ? { conditions: data.basis.conditions } : {}),
                ...(data.basis.versions ? { versions: data.basis.versions } : {}),
                ...(data.basis.observed_at ? { observedAt: data.basis.observed_at } : {}),
              },
            }
          : {}),
        generalKnowledge: !!data.general_knowledge,
      };
    case "evidence_collection":
      return {
        kind: "sources",
        summary: data.summary,
        subjects: subjects(data.subjects),
        entries: (data.entries ?? []).flatMap((entry) => {
          const [evidence] = refs([entry.evidence]);
          return evidence
            ? [
                {
                  evidence,
                  title: entry.title,
                  role: entry.role,
                  ...(entry.subject !== null && entry.subject !== undefined
                    ? { subject: entry.subject }
                    : {}),
                },
              ]
            : [];
        }),
      };
    case "comparison_matrix":
      return {
        kind: "matrix",
        subjects: subjects(data.subjects),
        criteria: data.criteria.map((criterion) => ({
          name: criterion.name,
          kind: criterion.kind.kind,
          ...(criterion.kind.kind === "measurement"
            ? { unit: criterion.kind.unit, basis: criterion.kind.basis }
            : criterion.kind.kind === "rating"
              ? { rubric: criterion.kind.rubric, scaleMax: criterion.kind.scale_max }
              : {}),
        })),
        cells: data.cells.map((row) =>
          row.map((cell) => ({
            value:
              cell.value.kind === "money"
                ? {
                    kind: "money",
                    amount: cell.value.amount,
                    currency: cell.value.currency,
                    ...(cell.value.observed_at ? { observedAt: cell.value.observed_at } : {}),
                  }
                : cell.value,
            evidence: refs(cell.evidence),
            ...(cell.note ? { note: cell.note } : {}),
            generalKnowledge: !!cell.general_knowledge,
          })),
        ),
        notes: data.notes ?? [],
      };
    case "findings":
      return {
        kind: "findings",
        subjects: subjects(data.subjects),
        items: data.items.map((item) => ({
          claim: item.claim,
          ...(item.subject !== null && item.subject !== undefined ? { subject: item.subject } : {}),
          evidence: refs(item.evidence),
          confidence: item.confidence,
          ...(item.detail ? { detail: item.detail } : {}),
          generalKnowledge: !!item.general_knowledge,
        })),
      };
    case "browser_resource_preview":
      return { kind: "browser", title: data.title, location: data.url, summary: data.summary };
  }
}

/**
 * Recognizable source labels: the citation's title, else its site, else what
 * kind of source it is. Never a number: "Source 10" tells a person nothing.
 */
function evidenceReferences(
  links: readonly WorkEvidenceLink[],
  execution: WorkExecutionFact,
): EvidenceReference[] {
  return links.map((link) => {
    const key = `${link.extraction_id}:${link.source_id}`;
    const record = execution.provider_evidence?.find((record) => record.id === link.extraction_id);
    const citation = record?.evidence.citations[link.source_id - 1];
    const origin = displayHost(citation?.url);
    if (!citation?.url)
      // A granted folder is not a web address: a file cites its own name.
      for (const entry of execution.file_evidence ?? [])
        if (entry.id === link.extraction_id)
          return {
            key,
            label: entry.file.name,
            origin: fileFolder(entry.file.path),
            file: { record: entry.id, path: entry.file.path },
          };
    return {
      key,
      label: citation?.title?.trim() || origin || m.work_env_page(),
      ...(origin ? { origin } : {}),
      ...(citation?.url ? { url: citation.url } : {}),
    };
  });
}

export function artifactView(artifact: WorkArtifactV1, execution: WorkExecutionFact): ArtifactView {
  const user = execution.user_artifacts?.find((value) => value.artifact === artifact.id);
  const links = user?.edited_data ? user.evidence : artifact.evidence;
  const evidence = evidenceReferences(links, execution);
  const refs: Refs = (indices) =>
    (indices ?? []).flatMap((index) => (evidence[index] ? [evidence[index]] : []));
  return {
    key: artifact.id,
    title: artifact.title,
    content: content(user?.edited_data ?? artifact.data, refs),
    // A result reads as finished; only the person's own decision says more.
    reviewLabel:
      user?.decision === "accepted"
        ? m.work_accepted()
        : user?.decision === "rejected"
          ? m.work_rejected()
          : m.work_env_status_done(),
    evidence,
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
  const live = execution && isLive(state, execution);
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
          ? (activityLabel(signal.activity) ?? "")
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

const ACTIVITY_LABELS: Record<WorkSignalV1["activity"], () => string> = {
  planning: m.work_activity_planning,
  delegating: m.work_activity_delegating,
  searching: m.work_activity_searching,
  reading: m.work_activity_reading,
  interacting: m.work_activity_interacting,
  verifying: m.work_activity_verifying,
  recovering: m.work_activity_recovering,
  comparing: m.work_activity_comparing,
  producing_artifact: m.work_activity_producing,
  paused: m.work_activity_paused,
  waiting_for_approval: m.work_activity_approval,
  waiting_for_human: m.work_activity_human,
  cancelling: m.work_activity_cancelling,
  finishing: m.work_activity_finishing,
};
/** What a live attempt is doing, in a few words. */
export function activityLabel(activity: string): string | undefined {
  return (ACTIVITY_LABELS as Record<string, (() => string) | undefined>)[activity]?.();
}

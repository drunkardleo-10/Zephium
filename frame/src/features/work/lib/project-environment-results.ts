import type {
  WorkEnvironmentReference,
  WorkEnvironmentSnapshot,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import type { CanvasItem } from "./canvas-model";
import { artifactView } from "./project-work";
import { isAgentExecution } from "./agent-steps";
import * as m from "$shared/i18n/messages";
export type ResultReference = Extract<WorkEnvironmentReference, { kind: "artifact" }>;
/** Exact transient result projections; only an explicit save creates an environment element. */
export function environmentResults(
  snapshot: WorkEnvironmentSnapshot,
  base: readonly CanvasItem[],
  state: WorkRuntimeProjection | null,
) {
  const items = [...base];
  let remaining: { objective: string; execution: string; count: number } | null = null;
  const references = new Map<string, ResultReference>();
  for (const element of snapshot.elements)
    if (element.reference.kind === "artifact") references.set(element.id, element.reference);
  if (
    !state ||
    !snapshot.elements.some(
      (element) =>
        element.reference.kind === "objective" && element.reference.objective === state.work.id,
    )
  )
    return { items, references, remaining };
  const execution = state.executions.at(-1);
  // Agent runs land as real objects while they run; nothing transient here.
  if (!execution || isAgentExecution(execution)) return { items, references, remaining };
  const attached = new Set(
    [...references.values()]
      .filter(
        (reference) =>
          reference.objective === state.work.id && reference.execution === execution.id,
      )
      .map((reference) => reference.artifact),
  );
  const unplaced = execution.artifacts.filter((artifact) => !attached.has(artifact.id));
  const capacity = Math.max(0, Math.min(12, 500 - items.length));
  const selected = capacity
    ? unplaced
        .filter(
          (artifact) =>
            artifact.execution === execution.id &&
            execution.spec.nodes.some(
              (assignment) => assignment.node === artifact.node && assignment.parent === null,
            ),
        )
        .slice(-capacity)
    : [];
  const hidden = unplaced.length - selected.length;
  if (hidden) remaining = { objective: state.work.id, execution: execution.id, count: hidden };
  for (const artifact of selected) {
    const id = `result:${execution.id}:${artifact.id}`;
    const view = artifactView(artifact, execution);
    references.set(id, {
      kind: "artifact",
      objective: state.work.id,
      execution: execution.id,
      artifact: artifact.id,
    });
    items.push({
      id,
      type: "result",
      title: artifact.title,
      kind: m.work_env_result(),
      detail: "",
      status: view.reviewLabel,
      artifact: view,
      layout: "artifact",
      actionLabel: m.work_env_add_result(),
    });
  }
  return { items, references, remaining };
}

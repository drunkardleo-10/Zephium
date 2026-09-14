import type {
  WorkEnvironmentSnapshot,
  WorkRuntimeProjection,
  WorkPlanRevision,
} from "$shared/ipc/bindings";
import type { CanvasItem, CanvasLink, CanvasPosition } from "./canvas-model";
import { projectWork } from "./project-work";
import * as m from "$shared/i18n/messages";

/** One expanded objective at a time. Derived IDs and geometry never become Store entities. */
export function environmentPlan(
  snapshot: WorkEnvironmentSnapshot,
  base: readonly CanvasItem[],
  objectives: ReadonlyMap<string, WorkRuntimeProjection>,
  plans: ReadonlyMap<string, WorkPlanRevision>,
  expanded: string | null,
) {
  const items = base.map((item) => ({ ...item }));
  const links: CanvasLink[] = [];
  const targets = new Map<string, { objective: string; execution: string | null }>();
  const positions: Record<string, CanvasPosition> = {};
  const selected = expanded;
  for (const [index, element] of snapshot.elements.entries()) {
    if (element.reference.kind !== "objective") continue;
    const state = objectives.get(element.reference.objective);
    const latest = state?.executions.at(-1);
    const newerDraft =
      state?.work.plan &&
      latest &&
      state.work.plan.revision !== latest.spec.plan_revision &&
      ["completed", "needs_review", "cancelled", "failed", "interrupted"].includes(latest.status);
    const execution = newerDraft ? undefined : latest;
    const plan = execution ? plans.get(element.reference.objective) : state?.work.plan;
    const exact =
      plan &&
      (execution
        ? plan.revision === execution.spec.plan_revision
        : plan.revision === state?.work.plan?.revision);
    const item = items.find((item) => item.id === element.id);
    if (!state || !item || !exact) continue;
    const projected = projectWork(state, plan, execution?.id ?? null);
    const nodes = projected.items.filter((item) => !item.artifactKey);
    const ids = new Set(nodes.map((node) => node.id));
    const edges = projected.links.filter((link) => ids.has(link.source) && ids.has(link.target));
    if (!nodes.length) continue;
    item.detail =
      (execution
        ? m.work_env_execution_plan({ revision: plan.revision })
        : m.work_env_current_plan({ revision: plan.revision })) +
      "\n" +
      m.work_env_plan_summary({
        steps: nodes.length,
        results: execution?.artifacts.length ?? 0,
      });
    if (
      base.length + nodes.length > 500 ||
      nodes.length > 64 ||
      edges.length + nodes.length > 2000
    ) {
      item.detail += "\n" + m.work_env_plan_capacity();
      continue;
    }
    item.actionLabel =
      selected === element.id ? m.work_env_collapse_plan() : m.work_env_show_plan();
    if (selected !== element.id) continue;
    const key = (node: string) => `plan:${element.id}:${plan.revision}:${node}`;
    const anchor = snapshot.view.placements.find((place) => place.element === element.id) ?? {
      x: 80 + (index % 3) * 520,
      y: 120 + Math.floor(index / 3) * 420,
    };
    const levels = new Map<string, number>();
    function level(id: string, depth = 0): number {
      if (levels.has(id)) return levels.get(id)!;
      const node = plan?.draft.nodes.find((node) => node.id === id);
      const value =
        depth >= 64
          ? 0
          : Math.max(0, ...(node?.dependencies ?? []).map((id) => level(id, depth + 1) + 1));
      levels.set(id, value);
      return value;
    }
    const rows = new Map<number, number>();
    for (const node of nodes) {
      const id = key(node.id);
      items.push({
        ...node,
        id,
        title: node.title.slice(0, 120),
        detail: "",
        responsibility: {
          outputs:
            plan.draft.nodes
              .find((entry) => entry.id === node.id)
              ?.outputs.map((output) => output.name) ?? [],
        },
      });
      if (!plan.draft.nodes.find((entry) => entry.id === node.id)?.dependencies.length)
        links.push({
          id: `plan-root:${element.id}:${node.id}`,
          source: element.id,
          target: id,
          kind: "reference",
        });
      targets.set(id, { objective: state.work.id, execution: execution?.id ?? null });
      const column = level(node.id);
      const row = rows.get(column) ?? 0;
      rows.set(column, row + 1);
      positions[id] = { x: anchor.x + 360 + column * 360, y: anchor.y + row * 220 };
    }
    // Reserve the actual resource cards before placing derived responsibility groups.
    // This runs only for initial geometry; reconciliation preserves user positions.
    const obstacles = snapshot.elements.map((entry, index) => {
      const place = snapshot.view.placements.find((place) => place.element === entry.id);
      return (
        place ?? {
          x: 80 + (index % 3) * 520,
          y: 120 + Math.floor(index / 3) * 420,
          width: base.find((item) => item.id === entry.id)?.layout === "artifact" ? 480 : 280,
          height: base.find((item) => item.id === entry.id)?.layout === "artifact" ? 360 : 160,
        }
      );
    });
    const group = nodes.map((node) => positions[key(node.id)]!);
    let shift = 0;
    while (
      group.some((point) =>
        obstacles.some(
          (other) =>
            point.x < other.x + other.width + 40 &&
            point.x + 320 > other.x &&
            point.y + shift < other.y + other.height + 40 &&
            point.y + shift + 200 > other.y,
        ),
      )
    )
      shift += 220;
    for (const point of group) point.y += shift;
    links.push(
      ...edges.map((link, index) => ({
        ...link,
        id: `plan-link:${element.id}:${plan.revision}:${index}`,
        source: key(link.source),
        target: key(link.target),
      })),
    );
  }
  return { items, links, targets, positions };
}

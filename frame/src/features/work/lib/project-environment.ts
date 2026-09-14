import type {
  WorkEnvironmentSnapshot,
  TabView,
  ResourceSummary,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import { artifactView } from "./project-work";
import type { CanvasItem, CanvasView } from "./canvas-model";
import * as m from "$shared/i18n/messages";

function objectiveStatus(projection: WorkRuntimeProjection): string {
  const execution = projection.executions.at(-1);
  if (execution) {
    if (projection.interrupted.includes(execution.id)) return m.work_interrupted();
    const labels = {
      approved: m.work_env_status_approved,
      running: m.work_env_status_running,
      cancel_requested: m.work_env_status_stopping,
      completed: m.work_env_status_completed,
      needs_review: m.work_review_required,
      cancelled: m.work_env_status_cancelled,
      failed: m.work_env_status_failed,
      interrupted: m.work_interrupted,
    };
    return labels[execution.status]();
  }
  return projection.work.status === "draft"
    ? m.work_env_status_draft()
    : projection.work.status === "needs_input"
      ? m.work_env_status_needs_input()
      : m.work_env_status_plan_ready();
}

export function environmentItems(
  snapshot: WorkEnvironmentSnapshot,
  tabs: readonly TabView[],
  notes: readonly ResourceSummary[],
  objectives: ReadonlyMap<string, WorkRuntimeProjection> = new Map(),
): CanvasItem[] {
  return snapshot.elements.map((element) => {
    const area = snapshot.areas.find((area) => area.id === element.area)?.title ?? "";
    if (element.reference.kind === "browser") {
      const tabId = element.reference.tab;
      const tab = tabs.find((tab) => tab.id === tabId);
      let origin = "";
      try {
        if (tab?.url) {
          const parsed = new URL(tab.url);
          if (parsed.origin !== "null") origin = parsed.origin;
        }
      } catch {
        /* Invalid projection text stays inert. */
      }
      return {
        id: element.id,
        type: "tab",
        area: element.area,
        kind: m.work_env_browser_resource(),
        title: tab?.title || m.work_env_unavailable_tab(),
        detail: origin,
        status: tab ? area : m.work_env_tab_unavailable(),
        favicon: tab?.favicon ?? null,
        unavailable: !tab,
      };
    }
    if (element.reference.kind === "resource") {
      const resourceId = element.reference.resource;
      const note = notes.find((note) => note.id === resourceId);
      return {
        id: element.id,
        type: "note",
        area: element.area,
        kind: m.work_env_notes(),
        title: note?.title || m.work_env_saved_resource(),
        detail: note?.updated_at ?? "",
        status: area,
        unavailable: !note,
      };
    }
    const projection = objectives.get(element.reference.objective);
    if (element.reference.kind === "artifact") {
      const reference = element.reference;
      const execution = projection?.executions.find(
        (execution) => execution.id === reference.execution,
      );
      const artifact = execution?.artifacts.find((artifact) => artifact.id === reference.artifact);
      const view = artifact && execution ? artifactView(artifact, execution) : undefined;
      return {
        id: element.id,
        type: "result",
        area: element.area,
        kind: m.work_env_result(),
        title: artifact?.title ?? m.work_artifact_unavailable(),
        detail: "",
        status: view?.reviewLabel ?? m.work_env_open_to_load(),
        artifact: view,
        layout: "artifact",
      };
    }
    return {
      id: element.id,
      type: "objective",
      area: element.area,
      kind: m.work_env_objective(),
      title: projection?.work.objective.slice(0, 512) ?? m.work_env_objective(),
      detail: projection
        ? m.work_env_objective_status({
            status: objectiveStatus(projection),
          })
        : m.work_env_open_to_load(),
      status: area,
    };
  });
}
export function environmentView(snapshot: WorkEnvironmentSnapshot): CanvasView {
  return {
    areas: Object.fromEntries(
      (snapshot.view.areas ?? []).map((place) => [
        place.area,
        { x: place.x, y: place.y, width: place.width, height: place.height },
      ]),
    ),
    sizes: Object.fromEntries(
      snapshot.view.placements.map((place) => [
        place.element,
        { width: place.width, height: place.height },
      ]),
    ),
    positions: Object.fromEntries(
      snapshot.view.placements.map((place) => [place.element, { x: place.x, y: place.y }]),
    ),
    viewport: { x: snapshot.view.x, y: snapshot.view.y, zoom: snapshot.view.zoom_milli / 1000 },
  };
}

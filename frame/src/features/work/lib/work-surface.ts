import type { ArtifactView } from "$shared/ui/data/Artifact";
import { validScene, type CanvasItem, type CanvasLink } from "./canvas-model";

/** A presentation supplied by a host. This is neither WorkProjectionV1 nor a frontend store. */
export type WorkSurfaceView = {
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
/** Local UI events. A live host must bind these to exact runtime-owned command operands. */
export type WorkSurfaceIntent =
  | { kind: "objective"; text: string }
  | { kind: "answer"; question: string; text: string }
  | { kind: "action"; key: string };
export type WorkRequestView = {
  state: "ready" | "pending" | "rejected" | "conflict" | "unknown" | "resynchronizing";
  message: string;
};
export function surfaceRenderable(view: WorkSurfaceView): boolean {
  const unique = (keys: readonly string[]) =>
    keys.every((key) => key.length > 0 && key.length <= 128) && new Set(keys).size === keys.length;
  return (
    validScene(view.items, view.links) &&
    (!view.notice || (view.notice.title.length <= 512 && view.notice.detail.length <= 4096)) &&
    view.title.length <= 512 &&
    view.objective.length <= 16_384 &&
    view.phase.length <= 256 &&
    view.questions.length <= 32 &&
    unique(view.questions.map((q) => q.key)) &&
    view.questions.every(
      (q) =>
        q.prompt.length <= 4096 &&
        q.options.length <= 32 &&
        q.options.every((option) => option.length <= 2048),
    ) &&
    view.actions.length <= 32 &&
    unique(view.actions.map((a) => a.key)) &&
    view.actions.every(
      (a) => a.label.length <= 256 && a.scope.length <= 4096 && a.consequence.length <= 4096,
    ) &&
    view.artifacts.length <= 256 &&
    unique(view.artifacts.map((a) => a.key))
  );
}

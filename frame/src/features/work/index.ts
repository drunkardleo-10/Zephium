export const loadWorkCanvas = () => import("./components/WorkCanvas.svelte");
export const loadWorkSurface = () => import("./components/WorkSurface.svelte");
export type { CanvasView } from "./lib/canvas-model";
export type { WorkSurfaceView, WorkSurfaceIntent, WorkRequestView } from "./lib/work-surface";

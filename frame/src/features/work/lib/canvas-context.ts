/** Instance-local UI callbacks; never part of a saved canvas node or runtime projection. */
export const canvasInspection = Symbol("canvas-inspection");
export const canvasResize = Symbol("canvas-resize");
export const canvasAction = Symbol("canvas-action");
export const canvasEvidence = Symbol("canvas-evidence");
export const canvasFocusResult = Symbol("canvas-focus-result");
export const canvasOpen = Symbol("canvas-open");
export const canvasOpenLink = Symbol("canvas-open-link");
export const canvasAreas = Symbol("canvas-areas");
/** The person whose words the request card carries. */
export const canvasAuthor = Symbol("canvas-author");
/** The admitted picture of each subject, by merge key, for compare columns. */
export const canvasPictures = Symbol("canvas-pictures");
/** An area's own actions: fit to members, rename in place, remove. */
export const canvasAreaActions = Symbol("canvas-area-actions");
/** How a node that just appeared arrives: with a group (`page`), into one (`base`), or not. */
export const canvasArrival = Symbol("canvas-arrival");
/** A diagram part's rename, where the result can still be corrected. */
export const canvasRename = Symbol("canvas-rename");
/** Asks native once for the icon of an origin no tab has shown. */
export const canvasProbe = Symbol("canvas-probe");
/** Selects a group's members at once: a diagram taken whole. */
export const canvasSelectGroup = Symbol("canvas-select-group");

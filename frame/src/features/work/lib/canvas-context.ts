import type { EvidenceReference } from "$shared/ui/data/Artifact";

/** Instance-local UI callbacks; never part of a saved canvas node or runtime projection. */
export const canvasInspection = Symbol("canvas-inspection");
export const canvasResize = Symbol("canvas-resize");
export const canvasAction = Symbol("canvas-action");
export const canvasEvidence = Symbol("canvas-evidence");
export const canvasFocusResult = Symbol("canvas-focus-result");
export const canvasOpen = Symbol("canvas-open");
export const canvasOpenLink = Symbol("canvas-open-link");
export const canvasAreas = Symbol("canvas-areas");
/** The admitted picture of each subject, by merge key, for compare columns. */
export const canvasPictures = Symbol("canvas-pictures");
/** An area's own actions: fit to members, rename in place, remove. */
export const canvasAreaActions = Symbol("canvas-area-actions");
/** How a node that just appeared arrives: with a group (`page`), into one (`base`), or not. */
export const canvasArrival = Symbol("canvas-arrival");
/** Asks native once for the icon of an origin no tab has shown. */
export const canvasProbe = Symbol("canvas-probe");
/** The runs behind the canvas, for a helper's own view of a part: `(objective) => projection`. */
export const canvasWork = Symbol("canvas-work");
/**
 * Whether a node has come near the view: until it has, it stands as an empty
 * box of its size, so opening a large work draws only what can be seen.
 */
export const canvasSeen = Symbol("canvas-seen");
/** Whether the canvas is seen from far (under half size): page frames draw as small copies. */
export const canvasFar = Symbol("canvas-far");
/** What a board's blocks ask of their canvas: see `BoardActions`. */
export const canvasBoard = Symbol("canvas-board");
/** A board's blocks report their size, open in place, and act through the canvas's owner. */
export type BoardActions = {
  /** A block's natural height at the width it stands at, open or not. */
  measure: (id: string, width: number, open: boolean, height: number) => void;
  /** Opens a block in place, or closes the one that is open. */
  toggle: (id: string) => void;
  /** The composer takes a question about something on the board. */
  ask: (name: string) => void;
  /** Chooses an entity, or takes the choice back. */
  choose: (element: string, chosen: boolean) => void;
  /** A source chip opens its page, or the file it names. */
  evidence: (reference: EvidenceReference) => void;
  /** An entity's whole product view. */
  entity: (element: string) => void;
  /** A trail's command, as the run recorded it. */
  command: (record: string) => void;
  /** A page, opened in the pane. */
  page: (url: string) => void;
  /** Picks laid side by side as a sheet, in the centre. */
  compare?: (id: string) => void;
  /** A draft's Send: always through Confirm. */
  send?: (id: string) => void;
  /** A note's new Markdown, written through the notes store. */
  write?: (id: string, markdown: string) => void;
  /** Save as note until the note exists, then Open note; nothing for what has no text. */
  note: (id: string) => { label: string; disabled?: boolean; onclick?: () => void } | undefined;
};

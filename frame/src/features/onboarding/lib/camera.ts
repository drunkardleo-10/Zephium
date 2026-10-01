/** The stage onboarding composes on, in its own pixels; the whole of it is
 *  scaled to the window, so every scene keeps its proportions. */
export const STAGE = { width: 1240, height: 780 } as const;

/** The window drawn on the stage, at its own full size. */
export const WINDOW = { width: 880, height: 560 } as const;

export type Shot = {
  /** How large the window is drawn. */
  scale: number;
  /** The point of the window to frame, in the window's pixels. */
  focus: readonly [number, number];
  /** Where on the stage that point lands. */
  at: readonly [number, number];
  /** How far down the window, in its pixels, it is drawn before it has
   *  faded into the ground; a shot without one shows all of it. */
  fade?: number;
};

const CENTRE = [WINDOW.width / 2, WINDOW.height / 2] as const;

/** One framing of the window per scene. Each keeps clear of the heading
 *  above it and the controls below it, and neighbours differ, so moving on
 *  is always a move. */
export const SHOTS = {
  // Only its top edge, rising into the bottom of the welcome.
  peek: { scale: 0.9, focus: [440, 0], at: [620, 652], fade: 180 },
  // Risen a little further, still waiting below the question.
  you: { scale: 0.78, focus: [440, 0], at: [620, 430], fade: 300 },
  // Whole, to the left, so the folder an import brings lands in plain view.
  import: { scale: 0.7, focus: CENTRE, at: [400, 468] },
  // Crossing to the right and closing in on the dock.
  essentials: { scale: 0.78, focus: [230, 410], at: [846, 560] },
  // Lowered out of the frame while the launcher has the stage.
  launcher: { scale: 0.7, focus: CENTRE, at: [620, 1180] },
  work: { scale: 0.82, focus: CENTRE, at: [620, 452] },
  ready: { scale: 0.7, focus: CENTRE, at: [620, 462] },
} as const satisfies Record<string, Shot>;

/** The transform that puts a shot's focus where it belongs, the window
 *  being drawn from its own top left and scaled about its centre. */
export function frame(shot: Shot): string {
  const [cx, cy] = CENTRE;
  const x = shot.at[0] - cx - shot.scale * (shot.focus[0] - cx);
  const y = shot.at[1] - cy - shot.scale * (shot.focus[1] - cy);
  return `translate(${round(x)}px, ${round(y)}px) scale(${round(shot.scale, 4)})`;
}

const round = (value: number, places = 2) => Math.round(value * 10 ** places) / 10 ** places;
